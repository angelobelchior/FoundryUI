# Spike — SDK `foundry-local-sdk` vs CLI (Fase 0)

## Ambiente validado

- CLI instalado: `foundry` **0.13.0** (Homebrew `foundrylocal`).
- Runtime nativo: `libfoundry_local.dylib` = **Microsoft.AI.Foundry.Local.Runtime 2.3.0**
  (`/opt/homebrew/Cellar/foundrylocal/0.13.0/libexec/`, com `libonnxruntime*.dylib`).
- SDK testado: `foundry-local-sdk` **2.3.1** (casa com o runtime 2.3.0).
- Cache customizado: `/Volumes/Dados/.foundry/cache/models` (`~/.foundry/config.json`).
- Crate isolado em `spikes/agent_wave7/spike/` (não toca no `Cargo.toml` do app).

## Resultados confirmados (in-process)

1. **Carregamento do runtime nativo via `library_path` — OK.** O SDK carrega
   `libfoundry_local.dylib` 2.3.0 sem recompilar/embarcar nada (resolve em runtime).
2. **Catálogo idêntico ao CLI — 48 modelos**, batendo com `catalog/overlay.toml` (48).
3. **`get_cached_models()` = 2** (qwen3-embedding-8b, whisper-large-v3-turbo), correto.
4. **`get_loaded_models()` = 0** com daemon parado, correto.
5. **Progresso/cancelamento** disponíveis via `download_builder().progress().cancel()`.

## Mapeamento de campos (SDK `ModelInfo` → `ModelSummary` do app)

| app `ModelSummary` | fonte no SDK | status |
|---|---|---|
| `alias` | `info.alias` | ✓ direto |
| `id` | `info.id` | ✓ direto |
| `display_name` | `info.display_name` | ✓ direto |
| `model_type` (Chat/Speech/Embedding/Multimodal) | **não é** `info.model_type` (= "ONNX", formato); usar `info.task` | ⚠️ mapear task→kind |
| `device` (Cpu/Gpu/Npu) | `info.runtime.device_type` (`DeviceType`) | ✓ enum→string |
| `file_size_mb` (`f64`) | `info.file_size_mb` (`Option<u64>`) | ✓ converter |
| `cached` | `info.cached` | ✓ direto |
| `loaded` | comparar alias/id com `get_loaded_models()` | ✓ mesmo padrão atual |
| `license` | `info.license` | ✓ direto |
| `publisher` | `info.publisher` | ✓ direto |
| `supports_tool_calling` | `info.supports_tool_calling` | ✓ direto |
| `supports_reasoning` | `get_int_property("supports_reasoning", -1)` → `Option<bool>` | ⚠️ não está em `ModelInfo`; via property |
| `source` (Public vs local) | `Catalog::catalog_type()` (`Public`/`Local`) | ⚠️ mapear |
| `input_modalities` (`Vec`) | `info.input_modalities` (`"text,image"` → split `,`) | ✓ split |
| `variants` | `model.variants()` (`id`, `cached`, device, execution_provider, size) | ✓ |
| `context_length` | `info.context_length` (`None` nos dados; overlay supre) | ⚠️ |

### Divergências relevantes

- **`model_type`**: o CLI retorna `type` = "Chat"/"Speech"/"Embedding" (task type); o SDK
  retorna `model_type` = "ONNX" (formato). O task type correto está em `info.task`
  (valores ainda a confirmar no próximo run). A `classify()` do app já prioriza
  modalities (que o SDK fornece), então o impacto é baixo, mas o fallback precisa usar
  `task`, não `model_type`.
- **`supports_reasoning`** não é exposto em `ModelInfo` (o binding 2.3.1 não o lê em
  `build_model_info`), mas a property FFI `"supports_reasoning"` existe e é acessível via
  `Model::get_int_property`. `capabilities` veio `None` no catálogo atual.
- **`context_length`** veio `None` no catálogo live (o CLI também não o retorna em
  `model list`; o app já supre via `overlay.toml`).

## Aviso macOS (não bloqueante)

`Class MATStreamingSessionDelegate is implemented in both libonnxruntime.dylib and
libonnxruntime-genai.dylib / libfoundry_local.dylib` — o SDK pré-carrega ORT+GenAI e o
`libfoundry_local.dylib` do Homebrew também os linka, duplicando a classe ObjC. É warning;
registrar como risco de empacotamento (a distribuição embarcada deve evitar a duplicação).

## Resultado shared host (correção de premissa)

`use_shared_host(true)` **NÃO interoperou** com o daemon do CLI. Rodando com o daemon
`foundrylocald` ativo (pid 69766), o SDK 2.3.1 falhou já no `create()`:

```
Native { code: InvalidArgument, message: "bootstrap_descriptor.cc:153 ...
native host bootstrap file is missing or too large" }
```

### Por quê (mecanismos distintos)

| | CLI (0.13.0) | SDK shared host (2.3.1) |
|---|---|---|
| processo host | `foundrylocald` | host iniciado pelo próprio SDK |
| descoberta | `~/.foundry/daemon.json` (`web_urls`, `pid`) | "bootstrap descriptor" JSON |
| IPC de gerenciamento | `~/.foundry/daemon.sock` (Unix socket) | bootstrap + IPC próprio |
| inferência | `web_urls` (HTTP loopback) | — |

O SDK procura um **bootstrap descriptor file** que o daemon do CLI não cria (o CLI usa
`daemon.sock`). Logo, **"shared host" não preserva interoperabilidade com o CLI** — a
premissa original estava errada.

### O que o daemon do CLI expõe

- `web_urls` (HTTP loopback): OpenAI-compatível (`/v1/models`, `/v1/chat/completions`,
  `/v1/embeddings`, `/v1/audio/transcriptions`) — **sem** estado de "loaded" nem
  gerenciamento.
- `daemon.sock` (Unix socket): gerenciamento (model list/load/unload/download) e estado
  "loaded" — **protocolo não documentado** (protocol_version 3).
- `foundry model list --loaded` **inicia o daemon implicitamente** se parado.

## Conclusão e recomendação corrigida

Eliminar o CLI de forma coerente exige **migrar gerenciamento E inferência juntos**, pois
o estado "loaded" vive no daemon (acoplado). Duas rotas:

1. **Reengenharia do `daemon.sock`** (manter o daemon, só trocar o CLI): preserva o estado
   compartilhado atual, mas depende de protocolo fechado e mutável — alto risco.
2. **SDK in-process completo** (recomendado): usa `get_loaded_models()`/`is_loaded()` do
   próprio processo; descarta o daemon e o CLI. É o padrão "embedded" oficial do Foundry
   Local. A detecção de "modelo carregado" é preservada (escopo do app, não do CLI).

A migração vira: `SdkBackend` (gerenciamento) + clientes de inferência do SDK
(`ChatSession`/`EmbeddingsSession`/`AudioSession`, ou `start_web_service()` para manter o
caminho HTTP atual apontando para o serviço embutido). Ver README da onda para as fases.
