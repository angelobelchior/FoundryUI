# Onda 6 — Permissões, referências e navegação de projetos

Recorte aprovado: FUI-012, FUI-021 e FUI-027 em paralelo, com integração central dos arquivos compartilhados. Base de revisão: `fe3ab55`. Bancos, pastas e listeners de teste são temporários; a interface usa dados sintéticos e mocks do IPC nativo.

## Permissões por conversa

O serviço persistente de autorização compõe as decisões da política central com o contexto, as revisões e o estado atual da ação. Solicitações, respostas e concessões não são interpretações de texto do modelo. Propostas e intenções de despacho são tipos opacos do backend, sem desserialização de autoridade fornecida pelo frontend.

- Concessões válidas sobrevivem à reabertura e não passam a outra conversa.
- Operações correspondem a fingerprints SHA-256 de seus argumentos, ambiente e recursos completos, nunca a prefixos de comandos.
- Ações destrutivas e acessos externos a projetos não geram permissões lembradas.
- Pastas explicitamente selecionadas só se tornam raízes persistentes em conversas sem projeto, depois de confirmação nativa. Uma seleção pendente fica obsoleta se houver mudança de contexto, modo ou revogação.
- Responder uma solicitação não executa a ação. O gate revalida os fatos atuais e registra a intenção de despacho uma única vez dentro de uma transação imediata.
- Revogar antes do gate bloqueia o próximo despacho. Depois dele, o recibo identifica ações iniciadas ou incertas, sem inventar cancelamento ou recuperação.
- Negação de uma operação não pode ser contornada repetindo a mesma ação com outro call ID na mesma execução. Uma nova solicitação explícita do usuário inicia outra execução e pode ser avaliada novamente.

As prévias persistidas são descrições sanitizadas, não autoridade. Uma prévia incompleta não recebe aprovação pelo bridge nativo: a solicitação continua pendente com erro explícito, enquanto consulta, negação e revogação permanecem disponíveis. Nesta etapa, somente a seleção nativa de uma pasta fornece a prévia completa do alcance correspondente; resumos de comandos, HTTP, alvos opacos e referências de mudança sem diferenças concretas não são apresentados como consentimento completo. A interface completa de permissões e os apresentadores concretos pertencem às etapas próprias, incluindo FUI-028. As capacidades positivas dos testes são fixtures confiáveis sintéticas; não habilitam comandos, rede ou um executor real.

## Cópias de referência

Qualquer histórico consultável pode ser origem, inclusive conversas arquivadas, sem projeto ou com pasta indisponível. A captura possui conteúdo próprio, origem descritiva congelada, data e identificador. Alterar ou apagar a origem não muda uma cópia existente.

As cópias são somente leitura e evidência histórica. Não incluem outras cópias recursivamente, fatos privados de autorização, mídia não suportada ou direitos para acessar a pasta de origem. O contrato relata as omissões e avisa quando a origem pode estar parcial. Chamadas antigas saem no IPC como `historical_tool_call`, não como chamadas atuais executáveis.

Atravessar um limite de projeto exige aviso nativo usando um plano capturado no backend. Contextos alterados durante a confirmação invalidam o plano. Remover uma cópia também usa plano e confirmação vinculados ao destino; não remove a origem. Captura e remoção respeitam os bloqueios de execução do destino sem impedir a consulta de históricos.

O seletor visual de conversas anexadas pertence a FUI-030; montagem, seleção de trechos e orçamento do contexto ficam nas etapas próprias. Esta onda não injeta cópias automaticamente no prompt.

## Projetos no chat

A navegação existente passa a distinguir **Conversas** e **Projetos** na coluna contextual, sem redesenhar o shell. Projeto, pasta e modo ficam identificáveis. Cadastro, consulta, renomeação e remoção de metadados não dependem de um modelo carregado.

O seletor nativo mantém a pasta selecionada no backend sem criar um projeto. Depois, a interface permite confirmar/editar o nome inicial ou abrir um cadastro existente; só a confirmação persiste o projeto, com um token opaco de uso único. A transação informa se houve criação ou reaproveitamento, inclusive em corrida com outro cadastro. Cancelar descarta a seleção, nomes inválidos permitem correção e seleções expiradas exigem nova escolha. Movimento, remoção e relocalização também só são aplicados depois do resultado confirmado; cancelamento e erros não simulam sucesso. Uma pasta indisponível mantém o histórico legível e oferece localização explícita.

Os testes cobrem teclado, nomes e caminhos longos, escopo das listas/busca, cancelamento e estados de falha/ocupado. Capturas em 1440 e 960 px verificam os tamanhos desktop suportados. A observação em 390 px é exploratória, não uma afirmação de suporte móvel. Janelas nativas do macOS são simuladas no navegador; interação real com esses diálogos não é uma prova automatizada desta entrega.

## Integração e evidências

Esquemas independentes de autorização e referências são concatenados ao esquema de agente e participam do mesmo stamp de versão. Não há migração de compatibilidade nem abertura do banco real da aplicação para validar esta implementação. A política de recriação de esquemas incompatíveis continua a previamente autorizada.

Detalhes dos testes, revisões e limites ficam em [VERIFICATION.md](VERIFICATION.md). O direcionamento da extensão visual está em [UI-DIRECTION.md](UI-DIRECTION.md). Onda 7 e execução de ferramentas não são habilitadas por esta entrega.
