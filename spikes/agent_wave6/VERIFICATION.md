# Verificação da Onda 6

## Recorte

- Base aprovada: `fe3ab55`, incluindo somente módulos e integração desta onda.
- Três interfaces públicas confirmadas para TDD: autorização persistente, cópias independentes e fluxos visíveis de projetos.
- Agentes independentes por domínio, fragmentos de esquema separados e integração central do IPC/cliente e arquivos compartilhados.
- Alterações anteriores do usuário preservadas fora do commit. Sem push.
- macOS; distribuição, assinatura e equivalência em outras plataformas permanecem adiadas.

## Evidências focais

- Autorização: 36 testes focais aprovados, incluindo reabertura, revogação/despacho com duas conexões, isolamento por conversa, lembrar inelegível, mudanças de modo/contexto, versões/argumentos exatos, negação repetida com outro call ID, prévias incompletas e recusa após desaparecimento de pasta.
- Referências: 18 testes focais aprovados, incluindo independência da origem/destino, omissões, conteúdo malicioso, planos obsoletos, bloqueios do destino e identidade estável do banco após movimentação para `Arc`.
- Integração nativa: testes públicos para cadastro versus pasta já cadastrada, relocalização bloqueada antes de existir streaming legado, captura como evidência histórica e concessão de pasta pendente durante revogação.
- UI: fluxos públicos testados contra o IPC simulado; primeira rodada visual identificou uma lacuna de mock em `params_snapshot`, confirmada por regressão vermelho → verde. O mock foi corrigido sem esconder toasts nem alterar o produto para a captura.

Os testes focais se sobrepõem às suítes completas; não devem ser somados como provas independentes. Casos positivos de política usam fatos sintéticos do backend, sem promover capacidade de execução real.

## Renderização

Capturas do estado anterior: `checks/wave6/baseline/`. Capturas da integração: `checks/wave6/final/`, com métricas e notas de validade. Os dados são sintéticos. Larguras suportadas: 1440 e 960 px; 390 px é apenas exploração do shell desktop existente.

O detector mecânico foi executado uma vez nos alvos alterados e retornou `[]`. A revisão visual independente pediu correções locais de contraste no texto funcional novo e de foco nos campos de diálogo. Os tokens globais foram preservados; regressões calculadas no DOM verificam texto com contraste mínimo de 4,5:1 e contorno visível com 3:1, espessura e afastamento de 2 px. A rodada após as correções foi capturada novamente e inspecionada.

A revisão de comportamento identificou depois a necessidade de confirmar o nome inicial antes da persistência. O fluxo foi implementado em duas etapas, com plano nativo no backend e edição/confirmação do nome na interface; as novas telas de criação e abertura de cadastro existente têm capturas próprias.

A revisão completa de acabamento confirmou a preservação da composição e da identidade, e pediu consistência de foco também nos demais controles novos. O ajuste final usa uma classe compartilhada somente nos controles de projetos e uma extensão opcional de `Modal.className`, sem mudar o padrão dos outros diálogos. A revalidação encerrou com `disposition: ship` para esse ajuste de foco nos tamanhos desktop revisados, sem ampliar o veredito para certificação funcional ou de janelas nativas.

Evidência final: 38 capturas atuais com métricas, incluindo seis provas de navegação real por Tab/Shift+Tab; todas abertas e inspecionadas pelo capturador e revisor. As 28 cenas desktop suportadas não têm overflow documental; todas têm fontes carregadas, nenhum comando nativo simulado sem suporte e nenhum toast genérico de falha. A inspeção em 390 px permanece exploratória. Alvos adicionados depois da primeira inspeção receberam uma verificação mecânica suplementar, também `[]`, sem repetir varreduras de polimento dos alvos antigos.

## Standards

Primeira revisão independente identificou três P2 de correção e uma heurística de duplicação:

- Seleção explícita de conversa por busca/link podia ser perdida por uma segunda carga de escopo, que selecionava a primeira conversa.
- Prévias de alteração eram declaradas completas sem apresentar os conteúdos/diferenças correspondentes.
- Reabrir uma raiz antes de distinguir aprovação de recusa impedia negar uma solicitação cuja pasta havia desaparecido.

A repetição da instalação de lista/contexto/seleção foi apontada como possível duplicação, não violação documentada. O carregador compartilhado e os recibos vinculados à navegação encerraram o defeito original e a heurística. As duas correções de autorização também foram revalidadas sem achados remanescentes no backend.

A revisão seguinte identificou um P2 adicional: cancelamento falhava se a seleção de pasta já tivesse expirado/sido removida. O descarte nativo passou a ser idempotente, sem exigir banco instalado, enquanto confirmação continua rejeitando tokens ausentes. A regressão pública passou por vermelho → verde e foi revalidada independentemente.

**Encerramento Standards/correção:** 0 defeitos novos ou não resolvidos, 0 violações documentadas e 0 heurísticas remanescentes no recorte revisado. Os revisores fizeram leitura independente do índice, sem reapresentar resultados do integrador como testes próprios.

## Spec

Primeira revisão independente identificou um P1 e dois P2:

- Descrições sanitizadas incompletas de comandos/alvos não podiam ser anunciadas como prévias completas para consentimento.
- Uma resposta atrasada de criação podia instalar uma conversa de outro projeto no escopo visível atual.
- F1 exigia confirmação/edição do nome inicial antes de gravar o projeto, não apenas renomeação posterior.

Não foi apontado defeito concreto na implementação de FUI-021. O P1 de consentimento foi encerrado: somente prévias completas de pasta nativa permanecem aprováveis no bridge; resumos de comandos, HTTP ou alterações opacas continuam pendentes. Nenhuma decisão de reduzir a garantia de consentimento foi tomada.

Os dois P2 originais de interface foram encerrados com o carregamento vinculado à navegação e a confirmação de nome anterior à persistência. A rodada seguinte identificou descarte de seleção expirada e atualização tardia após relocalização. O descarte agora é idempotente; o carregador exige revisão explícita, capturada antes de aguardar, inclusive para A→B→A. Correções revalidadas independentemente, com regressões públicas de respostas controladas.

**Encerramento Spec:** 0 achados remanescentes em FUI-012, FUI-021 e FUI-027 no recorte revisado. O resultado não comprova janelas nativas reais ou um executor de produção.

## Suíte final e patch isolado

| Verificação final | Resultado |
|---|---|
| Suíte Rust completa no checkout de trabalho | **310 unitários + 41 de integração aprovados**; 0 falhas; 8 previamente ignorados |
| Suíte Rust completa do patch isolado sobre `git archive fe3ab55` | **304 unitários + 41 de integração aprovados**; 0 falhas; 8 previamente ignorados |
| `cargo clippy --offline --lib -- -D warnings` no patch isolado | Aprovado |
| `pnpm test:smoke` no checkout | Build e **56 testes** aprovados: 39 de projetos, 5 de capturas e 12 anteriores |
| Regressões dos spikes com sandbox macOS | **21 aprovadas**, sem sandbox pulado |
| Typecheck/build/interface do patch frontend isolado | Aprovados, com **39 testes de projetos** em porta exclusiva, sem alterações anteriores do usuário |

As suítes não são somadas: o backend isolado tem **345 testes Rust aprovados**, enquanto o checkout com as alterações anteriores tem **351**. A diferença de seis testes unitários corresponde ao trabalho prévio do usuário preservado fora do commit.

O frontend isolado foi validado em uma porta separada para não reutilizar o servidor do checkout. A primeira tentativa identificou uma dependência de teste em um fluxo de onboarding anterior do usuário: aguardava incondicionalmente um botão que não existe no estado correspondente da base. Ela foi interrompida e não é apresentada como aprovação. O pré-requisito agora dispensa opcionalmente apenas o onboarding conhecido, sem esconder diálogos de projetos nem incluir alterações anteriores no produto. Os **39 testes passaram tanto no checkout quanto no frontend isolado** após essa correção; a verificação de parâmetros também abre o menu incumbente quando necessário, sem exigir o layout anterior do usuário.

A passagem independente de documentação em [DESIGN-CHECK.md](DESIGN-CHECK.md) conferiu a preservação do sistema existente. `PRODUCT.md` e o registro interno da superfície foram criados com aprovação do usuário e respeitam o gitignore de artefatos internos; não foram forçados para publicação. Não houve nova identidade visual, fonte, dependência ou asset de distribuição.

Nenhum teste ignorado foi apresentado como aprovado. A expiração real de dez minutos não foi esperada em relógio; regressões exercitam tokens removidos/evictados e o mesmo caminho de ausência, e a UI simula remoção real do token antes de confirmar Cancel/Escape. Dados de teste e capturas são sintéticos; o banco real da aplicação e modelos reais não foram utilizados.
