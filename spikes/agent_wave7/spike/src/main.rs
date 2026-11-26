use foundry_local_sdk::{DeviceType, FoundryLocalConfig, FoundryLocalManager};

const DEFAULT_LIB_DIR: &str = "/opt/homebrew/Cellar/foundrylocal/0.13.0/libexec";
const DEFAULT_CACHE_DIR: &str = "/Volumes/Dados/.foundry/cache/models";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lib_dir = std::env::var("FOUNDRY_LIB_DIR").unwrap_or_else(|_| DEFAULT_LIB_DIR.to_string());
    let cache_dir =
        std::env::var("FOUNDRY_CACHE_DIR").unwrap_or_else(|_| DEFAULT_CACHE_DIR.to_string());
    let shared = std::env::args().any(|a| a == "--shared");
    let websvc = std::env::args().any(|a| a == "--websvc");

    println!("== library_path = {lib_dir}");
    println!("== model_cache_dir = {cache_dir}");
    println!("== shared_host = {shared}");

    let config = FoundryLocalConfig::new("foundryui-spike")
        .library_path(&lib_dir)
        .model_cache_dir(&cache_dir)
        .use_shared_host(shared);

    let manager = FoundryLocalManager::create(config)?;

    if websvc {
        println!("== starting web service ==");
        manager.start_web_service().await?;
        let urls = manager.urls()?;
        println!("urls: {urls:?}");
        if let Some(base) = urls.first() {
            let models = reqwest::get(format!("{base}/v1/models")).await?.text().await?;
            println!("GET /v1/models => {}", &models[..models.len().min(300)]);
        }
        return Ok(());
    }

    match manager.urls() {
        Ok(urls) => println!("urls (existing listener): {urls:?}"),
        Err(e) => println!("urls: err {e}"),
    }

    match manager.discover_eps() {
        Ok(eps) => println!(
            "eps: {:?}",
            eps.iter()
                .map(|e| (&e.name, e.is_registered))
                .collect::<Vec<_>>()
        ),
        Err(e) => println!("eps: err {e}"),
    }

    let catalog = manager.catalog();

    let loaded = catalog.get_loaded_models().await?;
    println!("loaded_models: {}", loaded.len());
    for m in &loaded {
        println!("  LOADED alias={} id={}", m.alias(), m.id());
    }

    let cached = catalog.get_cached_models().await?;
    println!("cached_models: {}", cached.len());
    for m in &cached {
        println!("  CACHED alias={} id={}", m.alias(), m.id());
    }

    let models = catalog.get_models().await?;
    println!("catalog_models: {}", models.len());
    for m in models.iter().take(6) {
        let info = m.info()?;
        let device = info.runtime.as_ref().map(|r| device_str(&r.device_type));
        let supports_reasoning = m
            .get_int_property("supports_reasoning", -1)
            .ok()
            .and_then(|v| match v {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            });
        println!("--- {}", m.alias());
        println!(
            "  id={} type={} task={:?} provider_type={} device={:?} file_size_mb={:?}",
            info.id, info.model_type, info.task, info.provider_type, device, info.file_size_mb
        );
        println!(
            "  cached={} publisher={:?} license={:?} display_name={:?}",
            info.cached, info.publisher, info.license, info.display_name
        );
        println!(
            "  supports_tool_calling={:?} supports_reasoning={:?}",
            info.supports_tool_calling, supports_reasoning
        );
        println!(
            "  input_modalities={:?} output_modalities={:?} capabilities={:?}",
            info.input_modalities, info.output_modalities, info.capabilities
        );
        println!("  context_length={:?}", info.context_length);
        println!("  variants={}", m.variants().len());
        for v in m.variants().iter().take(3) {
            let vi = v.info()?;
            println!(
                "    variant id={} cached={} device={:?} ep={:?} size={:?}",
                vi.id,
                vi.cached,
                vi.runtime.as_ref().map(|r| device_str(&r.device_type)),
                vi.runtime.as_ref().map(|r| r.execution_provider.as_str()),
                vi.file_size_mb
            );
        }
    }

    println!("== full catalog (alias | task | tool | reasoning | modalities) ==");
    for m in &models {
        let info = m.info()?;
        let reasoning = m
            .get_int_property("supports_reasoning", -1)
            .ok()
            .and_then(|v| match v {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            });
        println!(
            "  {:<32} task={:?} tool={:?} reason={:?} in={:?} out={:?}",
            m.alias(),
            info.task,
            info.supports_tool_calling,
            reasoning,
            info.input_modalities,
            info.output_modalities
        );
    }

    Ok(())
}

fn device_str(d: &DeviceType) -> &'static str {
    match d {
        DeviceType::CPU => "Cpu",
        DeviceType::GPU => "Gpu",
        DeviceType::NPU => "Npu",
        DeviceType::Invalid => "Invalid",
    }
}
