# Chat: organização de conversas e projetos

Modo: Operate. Extensão local da superfície existente, construída diretamente em código. Escopo confirmado pelo usuário: organizar conversas por projeto, identificar a pasta ativa e concluir o ciclo de vida sem operações físicas implícitas.

## Direction contract

**THESIS:** a organização deve revelar onde a conversa pertence e onde futuras ações podem atuar. O histórico continua sendo a área principal; esta extensão não cria uma tela promocional ou um painel de métricas.

**OWN-WORLD:** herdar os tokens, superfícies, tipografia, densidade, bordas, ícones e controles do chat atual. Nenhuma nova paleta, fonte, imagem ou dependência visual. Estados e foco usam o sistema existente.

**STORY:** escolher Conversas ou Projetos, abrir uma conversa no escopo correto, reconhecer a pasta e executar uma ação explícita. Pasta indisponível não impede ler o histórico. Cancelamento e falha do backend não simulam sucesso.

**FIRST VIEWPORT:** preservar o shell atual, a navegação principal e a coluna contextual. A coluna do chat distingue Conversas e Projetos; a área principal mantém o título da conversa, seu contexto de projeto/pasta e as mensagens. A mudança de seleção mantém esse vínculo visível, sem deslocar o compositor para um novo fluxo.

**FORM:** extensão da composição existente, sem rodada de novas identidades. `seed: none (unavailable)`. Conferir 1440 e 960 px, largura mínima desktop atual; registrar também a observação exploratória de 390 px sem prometer suporte móvel inexistente.

**FINISH:** unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Limites de verificação

Os testes de interface simulam o contrato IPC e as respostas dos seletores/confirmações nativos. Não substituem interação real com janelas do macOS. Capturas devem usar somente dados sintéticos e devem ser inspecionadas, inclusive estados vazio, indisponível, ocupado e menus abertos.
