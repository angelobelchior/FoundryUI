# FoundryUI

Desktop app (Tauri + React + Rust) para usar o Azure AI Foundry Local sem terminal.

## Plataformas e arquiteturas de build

Build nativo suportado para:

- macOS Apple Silicon: `aarch64-apple-darwin`
- Windows x64: `x86_64-pc-windows-msvc`
- Windows ARM64: `aarch64-pc-windows-msvc`
- Linux x64: `x86_64-unknown-linux-gnu`
- Linux ARM64: `aarch64-unknown-linux-gnu`

## Pré-requisitos

### Comuns (todas as plataformas)

- Node.js 22+
- pnpm 12.18+
- Rust (stable) via rustup

```bash
corepack enable
pnpm --version
rustup --version
rustc --version
```

### macOS (Apple Silicon)

- macOS 15+
- Xcode Command Line Tools

```bash
xcode-select --install
```

### Windows (x64 e ARM64)

- Windows 11/Server recente
- Visual Studio Build Tools com workload **Desktop development with C++**
- WebView2 Runtime (normalmente já presente no Windows 11)

### Linux (x64 e ARM64)

Dependências nativas (Ubuntu/Debian):

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libsoup-3.0-dev \
  libasound2-dev \
  patchelf
```

## Clonar e instalar dependências

```bash
git clone <URL_DO_REPO>
cd FoundryUI
pnpm install --frozen-lockfile
```

## Rodar em desenvolvimento

```bash
pnpm tauri dev
```

## Build local (release)

Build para a arquitetura da máquina atual:

```bash
pnpm tauri build
```

Build explícito por target:

```bash
# macOS Apple Silicon
pnpm tauri build --target aarch64-apple-darwin

# Windows x64
pnpm tauri build --target x86_64-pc-windows-msvc

# Windows ARM64
pnpm tauri build --target aarch64-pc-windows-msvc

# Linux x64
pnpm tauri build --target x86_64-unknown-linux-gnu

# Linux ARM64
pnpm tauri build --target aarch64-unknown-linux-gnu
```

## Onde ficam os artefatos

Todos os bundles ficam em:

`src-tauri/target/<target>/release/bundle/`

Arquivos mais comuns:

- macOS: `bundle/macos/FoundryUI.app`, `bundle/dmg/*.dmg`
- Windows: `bundle/nsis/*.exe`, `bundle/msi/*.msi`
- Linux: `bundle/appimage/*.AppImage` (podendo incluir `.deb`/`.rpm` conforme ambiente)

## GitHub Actions (build + release multi-plataforma)

Workflow: `.github/workflows/release-multi-platform.yml`

Ele compila e publica release com assets para:

- macOS ARM64 (`macos-15`)
- Windows x64 (`windows-2025`)
- Windows ARM64 (`windows-11-arm`)
- Linux x64 (`ubuntu-24.04`)
- Linux ARM64 (`ubuntu-24.04-arm`)

Disparo:

- automático ao criar tag `v*`
- manual via `workflow_dispatch`

Exemplo de release por tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Verificações recomendadas

```bash
pnpm typecheck
pnpm build
cd src-tauri && cargo clippy -- -D warnings
```

## Observações

- Build macOS atual é sem assinatura e sem notarização.
- O app usa o Foundry Local em runtime. Para instalar/configurar a CLI, siga a documentação oficial:
  - https://learn.microsoft.com/en-us/azure/ai-foundry/foundry-local/
