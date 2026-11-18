# Onda 5 — Ciclo de contexto e inferência local

Recorte: FUI-010 e FUI-018, com implementação paralela e integração central dos arquivos compartilhados. Base de revisão aprovada: `55011ee`. Foco macOS, validação de arquivos por operação e distribuição adiada.

## Frentes

- Ciclo de contexto: planos do backend para entrada, saída e troca de projeto; remoção com manutenção ou exclusão das conversas; retenção e repetição idempotente.
- Inferência local: URL numérica de loopback, identidade real do runtime/processo e associação do listener; prova opaca revalidada no envio, sem proxies herdados nem redirecionamentos.
- Anexos: preparação em memória e cópias persistidas como bytes no SQLite, excluídas na mesma transação dos registros da conversa. Não há limpeza de arquivos por nome.
- Integração: confirmação nativa com plano capturado, barreira de geração e transporte tipado nos pontos que enviam contexto.

Nenhuma ferramenta de agente, leitura automática de projeto ou navegação visual de projetos é habilitada por esta onda. Rede de ferramentas continua independente da inferência; aprovar uma URL não habilita provedor remoto.

## Dados e execução

Mover uma conversa preserva seu histórico e anexos, mas invalida as autorizações e instruções aprovadas da revisão anterior. Esse conteúdo antigo não é apagado retroativamente.

Remover um projeto não remove sua pasta física nem seus arquivos, seja mantendo conversas sem projeto ou excluindo os registros próprios das conversas. Cópias de referência em outras conversas são independentes da origem. Cada registro de anexo possui sua própria cópia dos bytes; excluir uma conversa não remove a cópia pertencente a outra.

A revisão encontrou uma corrida entre conferir um arquivo e apagá-lo por nome. O usuário confirmou manter a garantia estrita e ampliar esta onda para usar armazenamento transacional dos anexos. A solução anterior de `fstatat` seguido de `unlinkat` foi substituída, não promovida como prova de exclusão segura. Também foi removida a varredura de arquivos na inicialização.

Identificadores de anexo não são caminhos de arquivo nem autorizações para abri-los. O chat e o comando de leitura de imagens consultam os bytes pelo ID no banco, sem fallback ao sistema de arquivos. A remoção dos bytes e dos metadados ocorre dentro da transação do ciclo de contexto; não depende de uma fila de limpeza física posterior. Recibos idempotentes não guardam conteúdo de anexos nem caminhos brutos de arquivos.

Não há promessa de apagamento forense de blocos livres, logs físicos do SQLite ou backups externos. Também não há migração nem exclusão retroativa de arquivos de instalações anteriores: o banco real da aplicação não foi aberto e a pasta antiga de anexos não é varrida por esta implementação.

Cancelamento de uma geração não é prova de término. Mudanças e remoções devem aguardar a liberação da geração e também respeitar as barreiras persistentes de trabalho ativo/incerto e streaming legado. A operação não interrompe processos silenciosamente nem promete desfazer efeitos anteriores.

## Integração nativa

O cliente TypeScript oferece `projectApi.moveConversation` e `projectApi.remove`. A remoção oferece manter conversas antes da opção de excluí-las; a confirmação destrutiva é adicional. Não há parâmetro `confirmed` vindo do frontend. O comando legado `chat_delete_conversation` também passa pela confirmação do backend.

As confirmações usam textos fixos do backend nos idiomas existentes (`en`, `pt-BR`, `es`), identificam os alvos e distinguem exclusão permanente dos dados internos de preservação dos arquivos físicos. O frontend informa apenas o idioma, não o texto da aprovação. A navegação visual de projetos continua em FUI-027.

## Pontos de envio de contexto

A preparação de geração e as operações de embeddings obtêm um destino tipado em `ServerManager::verified_endpoint`, sem aceitar a string de descoberta como prova. O transporte de chat recebe a prova opaca, não uma URL livre.

Os pontos integrados incluem histórico/imagens do chat, vetorização de documentos, busca/comparação semântica, síntese de respostas com citações e transcrição de áudio importado. O fallback de transcrição também precisa passar pelo destino verificado. A checagem não fica restrita ao primeiro envio de uma conversa ou ao rótulo “local” da interface.

Uma revisão compartilhada da sessão invalida provas preparadas quando endpoint, PID, início do runtime ou estado de execução mudam. A revisão é capturada junto com o vínculo do servidor, evitando associar uma prova antiga à geração de uma sessão nova.

## Prova nativa e transporte

No macOS, o backend compara o executável real e o início do processo com a instalação selecionada, além de conferir o proprietário do listener. O endpoint deve ser HTTP de loopback numérico, com porta explícita; nomes DNS, credenciais e campos adicionais não são aceitos.

No despacho, o transporte abre TCP sem enviar HTTP, verifica que o runtime esperado possui a conexão estabelecida com aquele par exato de endereços/portas e só então envia a requisição pelo mesmo socket aberto. Não há proxy herdado, redirecionamento automático, reutilização de conexão ou tentativa alternativa para outro destino.

A inspeção nativa roda fora dos workers assíncronos, sob os limites de preparação/despacho. Cancelar enquanto a inspeção está pendente não inicia uma conexão depois do cancelamento. O streaming mantém prazo para os cabeçalhos e os limites existentes de primeiro token/inatividade, sem transformar esse prazo no tempo total de uma resposta longa.

Essa prova não autentica criptograficamente os bytes executados, não protege contra um sistema operacional comprometido e não promete impedir que um runtime já verificado e comprometido transfira seu socket ou execute outro programa depois da inspeção. Assinatura/distribuição permanecem adiadas conforme o recorte aprovado.

## Verificação

As duas interfaces públicas foram confirmadas para TDD, com bancos/diretórios temporários, listeners locais controlados e fatos de teste identificados como tais. Não há autorização para downloads de modelos ou uso de conteúdo real de projetos nos testes. Evidências, limitações, revisões e resultados finais ficam em [VERIFICATION.md](VERIFICATION.md).
