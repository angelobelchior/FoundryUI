# Onda 7 — Eliminar o CLI `foundry` (migrar para o SDK `foundry-local-sdk`)

## Objetivo

Remover a dependência do binário `foundry` (subprocesso) para gerenciamento E inferência,
passando a usar o SDK Rust oficial (`foundry-local-sdk`), **mantendo o padrão de detecção
de modelo carregado** (`get_loaded_models()` / `is_loaded()`).

Decisão de arquitetura: **SDK in-process** (padrão "embedded" oficial do Foundry Local).

> Correção após o spike: a opção "shared host" foi descartada — ela **não** interoperar
> com o daemon do CLI (mecanismos de IPC distintos: `daemon.sock` do CLI vs "bootstrap
> descriptor" do SDK). Ver `reports/sdk-spike.md`.

## Contexto

O app usa o Foundry Local em duas camadas, acopladas pelo daemon:

| Camada | Hoje | Destino |
|---|---|---|
| Gerenciamento | subprocesso `foundry <cmd>` → daemon | SDK `foundry-local-sdk` in-process |
| Inferência | HTTP ao daemon (`/v1/*`) | clientes do SDK (ou `start_web_service()`) |

O estado "loaded" vive no daemon; logo, gerenciamento e inferência migram **juntos**.

Runtime nesta máquina: `libfoundry_local.dylib` = Runtime **2.3.0**. SDK alvo: **2.3.1**.

## Mapeamento CLI → SDK

| CLI | SDK |
|---|---|
| `foundry model list` | `catalog().get_models()` |
| `foundry model list --loaded` | `catalog().get_loaded_models()` |
| `foundry model info <a> -v` | `catalog().get_model(a)` + `.variants()`/`.info()` |
| `foundry model download <a> --force` | `model.download_builder().progress(f).cancel(flag).run()` |
| `foundry model load` / `unload` | `model.load()` / `model.unload()` |
| `foundry cache list` | `catalog().get_cached_models()` |
| `foundry cache location` | `FoundryLocalConfig.model_cache_dir(...)` |
| `foundry cache clear <a>` | `model.remove_from_cache()` |
| `foundry cache clear` (todos) | iterar `get_cached_models()` → `remove_from_cache()` |
| `foundry model register` / `unregister` | `local_catalog().register_model()` / `unregister_model()` |
| `foundry server start` / `stop` | `start_web_service()` / `stop_web_service()` (opcional) |
| `foundry server logs` | ler `logs_dir` diretamente |
| `foundry config show/set/reset` | `FoundryLocalConfig` + `additional_setting(k,v)` |
| `foundry status` / `report` | `discover_eps()` + diagnóstico próprio |

## Fases

### Fase 0 — Spike (ver `reports/sdk-spike.md`)
- ✅ Carrega `libfoundry_local.dylib` via `library_path`; catálogo 48 modelos.
- ✅ Mapeamento de campos validado; `start_web_service()` sobe a API OpenAI in-process.
- ❌ shared host não interopera com o daemon do CLI → descartado (in-process).

### Fase 1 — `SdkBackend` (gerenciamento) — ✅
- `sdk_backend.rs`: `SdkBackend` + `FoundryBackend` + conversão + erro + descoberta de runtime.

### Fase 2 — Inferência — ✅
- `local_inference/` simplificado (HTTP direto; verificação nativa removida).
- `server.rs` (ServerManager) gerencia `start_web_service()`/`stop_web_service()`.

### Fase 3 — Download — ✅
- `download.rs` usa `download_builder().progress().cancel()`.

### Fase 4 — Config/registro/diagnósticos — ✅
- `config_*` → `FoundryLocalConfig` (fixo na inicialização); `model register/unregister` →
  `local_catalog()`; `status`/`report` → `discover_eps()`.

### Fase 5 — Remoção do CLI — ✅
- `cli.rs`, impl `CliBackend`, `local_inference/native` (verificação `lsof`/`proc_pidinfo`)
  removidos. `AppState` usa `SdkBackend`. Testes CLI-dependentes removidos.
- `cargo test` verde (469 lib + integração).

## Pendências fora do recorte Rust

1. **Empacotamento do runtime** — `discover_runtime_dir()` resolve o Homebrew
   `foundrylocal`; distribuição final precisa embarcar `foundry_local` + ORT via
   `FOUNDRY_LOCAL_RUNTIME_VERSION` no build.
2. **Texto/onboarding do frontend** — `Onboarding.tsx`, `config/foundry-install.json` e
   `onboardingInstallGuidance.ts` ainda falam em "CLI"; ajustar para "runtime".
3. **Mocks de smoke test** (`tests/smoke/helpers/installFoundryMock.ts`) — comandos
   preservados, mas o mock simula o binário `foundry`; revisar.

## Validações realizadas

- ✅ `detects_and_lists_the_catalog` (ignorado): `detect()` + `list_models()` + resolução
  por id de variante.
- ✅ `loads_and_transcribes_via_the_web_service` (ignorado): `model.load()` +
  `start_web_service()` + `speech::transcribe` (caminho real do app) com o
  `whisper-large-v3-turbo` em cache — **inferência ponta a ponta confirmada**.
- ✅ `loads_and_chats_via_the_web_service` (ignorado): chat com `gemma-4-e2b-it` sem tools.
- ✅ `gemma_agent_turn_falls_back_to_chat_without_tools` (ignorado): o agente envia tools,
  o modelo rejeita ("requires model-published tool markers") e o provider **retenta sem
  tools** — desbloqueia o chat com modelos que não suportam tool calling estruturado.

## Achado: tool calling estruturado

- ⚠️ A inferência usa o **id** do modelo (não o alias); o `speech::transcribe` já trata o
  contrato JSON da transcrição (multipart → fallback `filename`).
- ⚠️ O catálogo marca `supports_tool_calling: true` para modelos (ex. `gemma-4-e2b-it`)
  que **não** suportam o formato estruturado de tools do OpenAI — o runtime devolve
  "structured tool calling requires model-published opening and closing tool markers".
  Os marcadores (`tool_call_start`/`tool_call_end`) não vêm expostos no SDK. Correção:
  o `EndpointTurnProvider` retenta sem tools quando o erro menciona tools; o
  `bridge_final_answer` agora persiste o código/detalhe do erro (antes gravava "error"
  com `error_json` vazio).

## Riscos

1. **Empacotamento do runtime nativo** — sem CLI, o app precisa embarcar
   `foundry_local` + ORT (via `FOUNDRY_LOCAL_RUNTIME_VERSION` no build). Impacta
   assinatura/tamanho do pacote macOS.
2. **Reescrita de inferência** — migrar chat/embeddings/áudio para os clientes do SDK é
   a maior mudança; mitigação é o `start_web_service()` (mantém o código HTTP atual).
3. **Fidelidade de metadados** — `model_type`/`supports_reasoning`/`context_length` têm
   nuances (ver relatório); `overlay.toml` cobre parte.
4. **Sem interoperabilidade com o CLI** — após a migração, modelos carregados pelo CLI
   em paralelo não são vistos (consequência aceita da remoção do CLI).
5. **Aviso macOS de símbolo duplicado** (`MATStreamingSessionDelegate` em ORT/GenAI/foundry_local)
   — registrar e evitar na distribuição embarcada.
