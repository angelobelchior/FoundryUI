# Conferência visual local — Onda 6

> Nota posterior do integrador: as revisões de correção e Spec foram encerradas sem achados remanescentes após esta passagem. As suítes finais e o frontend isolado também foram aprovados; ver `VERIFICATION.md`. O relato abaixo conserva o escopo e as evidências da passagem independente, sem atribuir a ela testes que não executou.

## Escopo e resultado

Extensão **Operate** do chat existente, não um novo mundo visual nem uma alteração aprovada do sistema global. Este registro compara a implementação com o incumbente e documenta o acabamento; não substitui revisão de correção, certificação de acessibilidade integral ou interação real com janelas nativas.

O parecer final **SHIP** foi informado pelo revisor de acabamento para a composição, tipografia, material, fundo, prioridade do histórico e foco corrigido em **1440×900 e 960×640**, incluindo F1 (nome antes da persistência), pasta duplicada, renomeação somente no aplicativo e indisponibilidade nos três idiomas. A amostra inspecionada nesta passagem é consistente com esse parecer local, sem regressão material observada; não constitui um segundo parecer funcional independente. A revisão de código ainda finaliza regressões lógicas de relocalização/cancelamento, sem mudança prevista de markup; não se declara sua aprovação aqui.

## Fatos conferidos no código

- **Paleta e superfícies mantidas:** `src/styles/tokens.css` continua definindo janela clara (`--bg-window: #f5f6fa`), canvas/elevado brancos, texto principal (`#1d1d1f`), acento azul (`#0071e3`), bordas e estados tonais translúcidos, variantes escuras e preferências existentes. `src/styles/global.css` mapeia esses tokens para as utilidades sem introduzir uma paleta de projetos. O diff contra HEAD de tokens, `package.json` e `pnpm-lock.yaml` está vazio.
- **Tipografia mantida:** pilha `--font-ui` de sistema/SF Pro Text e `--font-code` monoespaçada; base de 13 px multiplicada pela densidade. Permanecem as opções tipográficas incumbentes. Os novos controles usam a escala compacta já presente: metadados de 10,5 px, ações de 11,5–12 px, nomes de 12,5 px e campos/instruções de 13 px. Não há nova fonte carregada por esses componentes.
- **Composição e material mantidos:** navegação e coluna contextual à esquerda, cabeçalho, contexto da pasta, histórico como área principal e compositor ao fundo. `ChatWorkspace.tsx` mantém `ContextList`, `MessageList` e `Composer`; as capturas, e não apenas os tokens de largura, sustentam a continuidade do shell. Itens usam `--item-radius`, `--item-pad-x/y`, bordas e `--shadow-thumb`; diálogos reutilizam canvas/elevado, borda, cantos e `--shadow-popover` do `Modal` existente.
- **Primitivos reaproveitados:** `ContextList`, `IconButton`, ícones existentes, `ListEmpty`, `Spinner` e `Modal`. `ProjectsPanel.tsx`, `ProjectContext.tsx`, `ProjectCreateDialog.tsx` e `ProjectMoveDialog.tsx` estendem essa linguagem; não há imagem decorativa, dependência visual ou componente externo novo.
- **Correções locais, sem nova regra global:** texto funcional crítico novo usa `text-primary` (o texto principal existente, não o acento). `projectFocus.ts` aplica contorno sólido de **2 px**, afastamento de **2 px** e cor `primary` aos novos controles. Diálogos de projetos optam por `projectDialogFocus` via `Modal.className`; essa prop opcional não altera o foco padrão dos demais consumidores. O foco global translúcido permanece como incumbente, não como recomendação para novos controles críticos.

### Rótulos e fronteiras locais

- **MetadataVsFilesystem:** as traduções de criação dizem que arquivos não serão alterados; renomear altera somente o nome no aplicativo, não a pasta; duplicata oferece abrir o projeto existente sem renomeá-lo. O diálogo novo apresenta nome editável e pasta estática; o modo duplicado não apresenta campo de nome.
- **cancelErrorNativeBoundary:** cancelar, fechar e erros têm saídas e mensagens explícitas; movimentação informa contexto anterior, invalidação de permissões e confirmação nativa. `projectErrors.ts` traduz códigos conhecidos, incluindo seletor indisponível. São contratos de interface/cópia, não prova da execução nativa nem da persistência correta.
- **Histórico primeiro:** a pasta indisponível expõe estado, ação Localizar e explicação sem substituir a conversa salva. Nome/caminho longos têm truncamento intencional com `title` na lista/contexto; o caminho no diálogo quebra linhas.

## Evidência renderizada conferida

Baselines abertos: `checks/wave6/baseline/chat-1440.png` e `chat-960.png`. Em `checks/wave6/final/`, foram abertos diretamente nesta passagem:

- `selected-project-1440.png`: shell, contexto e histórico preservados, compositor desobstruído;
- `create-project-name-1440.png` e `open-existing-project-960.png`: modos de nome inicial e duplicata visualmente distintos, foco visível e caminho somente leitura;
- `move-dialog-960.png` e `root-rename-dialog-960.png`: aviso de contexto, seletor/campo focado e raiz `/` preservada no contexto;
- `keyboard-project-navigation-960.png`, `keyboard-locate-folder-960.png` e `keyboard-dialog-close-960.png`: contornos visíveis sem recorte nos três tipos de controle;
- `unavailable-folder-pt-BR-960.png` e `unavailable-folder-es-960.png`: estado, ajuda, ação e histórico legíveis. As cenas equivalentes em inglês estão representadas por `keyboard-locate-folder-960.png`; isto não é uma auditoria exaustiva de tradução;
- `selected-project-390.png`: compressão/recorte do shell desktop, registrada apenas como exploração fora do mínimo suportado.

Os **38 JSONs** atuais em `checks/wave6/final/` foram agregados por leitura, sem executar testes: 28 cenas desktop sem overflow horizontal do documento; todas com fontes `loaded`, `unhandledCommands: []` e zero toast genérico de falha. Os seis registros `keyboard-*.json` (três alvos em duas larguras) registram `focusVisible: true`, contorno `rgb(29, 29, 31)`, largura/offset de 2 px. Isso não prova ausência de overflow interno ou cobertura de todos os controles.

## Evidência relatada, não reexecutada nesta passagem

`checks/wave6/final/CAPTURE-NOTES.md` registra build e cinco testes visuais aprovados, 38 pares PNG/JSON, seis cenas alcançadas por Tab/Shift+Tab e abertura de todas as imagens pelo responsável pelas capturas. Registra também regressões públicas de UI relatadas pelo agente: contraste de texto crítico **≥4,5:1** e foco **≥3:1**. Esta passagem conferiu código, rasters amostrais e métricas salvas; não recalculou razões de contraste nem rodou essas suítes.

O solicitante informa detector inicial nos cinco alvos com resultado `[]` e suplemento somente nos novos alvos após F1 (`ProjectCreateDialog`, `projectFocus`, `Modal`) também `[]`. Não houve nova execução do detector aqui. Detector vazio não equivale a revisão visual ou funcional completa.

## Limites e lacunas preservadas

- Idiomas mantidos: português brasileiro, inglês e espanhol. **390×844 é exploratório**, fora do mínimo desktop Tauri de 960 px; não existe prova de aplicativo iOS físico ou interface nativa móvel. Seletores/confirmações macOS foram simulados no IPC, não certificados visualmente no sistema operacional.
- Não há raster novo de distribuição nesta extensão. Os **40 PNGs** deste conjunto (38 atuais + duas baselines) são exclusivamente verificação com dados sintéticos; não são assets do produto. Nenhuma fonte, imagem ou dependência foi adicionada por esta passagem.
- `DESIGN.md` já não existia antes da tarefa. A lacuna global é registrada, não reparada: não foram criados `DESIGN.md`, `.impeccable/design.json` ou regras globais a partir de decisões locais. A menção genérica a `DESIGN.md` em FINISH nos documentos de direção fica como divergência documental preexistente, subordinada ao escopo local confirmado. `VERIFICATION.md` ainda contém pendências históricas de acabamento/F1; este relatório não o reescreve nem encerra pendências de correção.
- Não se canonizam o contraste/foco fracos anteriores nem a limitação exploratória de 390 px. Preferências/variantes globais não foram integralmente inspecionadas nesta passagem. A existência de alterações concorrentes em arquivos globais no checkout não autoriza atribuí-las a esta extensão.
- **Única escrita desta passagem:** `spikes/agent_wave6/DESIGN-CHECK.md`. Nenhuma escrita em UI, tokens, estilos globais, produto, assets, mocks, backend, configuração, testes ou índice; nenhum commit ou serviço iniciado.
