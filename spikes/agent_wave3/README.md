# Onda 3 — Persistência e identidade de recursos

FUI-007 e FUI-008, com foco macOS. São fundações Rust, sem novas rotas Tauri, ferramentas de agente, alterações visuais ou execução de comandos.

## Recorte confirmado durante a implementação

O usuário confirmou TDD nas interfaces públicas de persistência e recursos, revisão contra `dd28a2f` incluindo somente os trechos desta onda, e a seguinte decisão adicional:

**Manter contenção estrita:** se um recurso sair da pasta autorizada, não aceitar autorização persistente apenas por já existir um descritor aberto. Operações sem garantia comprovada continuam bloqueadas; identidade e persistência podem avançar.

Essa decisão não foi substituída silenciosamente por semântica de objeto fixado. O módulo de recursos não lê bytes de arquivos de trabalho nem cria arquivos enquanto a garantia estrita estiver indisponível.

## Implementação

| Área | Caminho | Comportamento |
|---|---|---|
| Persistência | `src-tauri/src/foundry/db/agent/` | Projetos, contexto, execuções, chamadas, decisões, claims, diário, snapshots e fatos tipados |
| Esquema | `src-tauri/agent-schema.sql` | Estruturas relacionais, ownership, índices, estados e validação de payloads |
| Integração SQLite | `src-tauri/src/foundry/db.rs` | Mesmo banco da aplicação, preparação/recovery e sequência compartilhada com mensagens |
| Recursos | `src-tauri/src/foundry/resources/` | Identidade nativa de diretórios, relações de ancestralidade, revogação e negação explícita de I/O estrito não comprovado |

## Persistência: interface pública

Entrada principal: `Database::agent_store()`.

- `start_run` é idempotente por conversa/chave da requisição; uma chave de revisão antiga não cria nova execução.
- `propose_call` guarda chamada estruturada, argumento sanitizado e fingerprint SHA-256 canônico; reutilização incompatível do ID é conflito.
- `decide_call` vincula decisão à ação, argumentos e revisão; não é uma implementação da política de permissões de FUI-011/FUI-012.
- `claim_call` grava início antes de devolver um claim opaco, impede segunda aquisição e bloqueia escritores de resultado incerto conhecidos naquela conversa.
- `complete_call` registra resultado e histórico na mesma transação; repetição igual é idempotente e resultado divergente é conflito.
- `events` preserva ordem entre intenção, decisão, início e resultado. `history` exclui eventos de autorização e contém mensagens/chamadas/resultados apropriados à cópia informacional.
- `capture_snapshot` guarda cópia própria, sem cascata pela conversa de origem; arquivos de mídia não copiados são contados como omitidos.
- `record_fact` representa fatos tipados de concessões, raízes, instruções, contexto, rede, propostas e recuperação. `facts` serve auditoria histórica; `active_facts` exige a revisão atual.
- `record_project` deduplica chaves de identidade produzidas pelo módulo nativo; os campos de identidade/caminho armazenados são hints, não direitos de acesso.

Argumentos e resumos de resultados persistidos são conservadoramente sanitizados: valores escalares são ocultados e apenas uma lista finita de rótulos de metadados é preservada. Chaves dinâmicas desconhecidas recebem rótulos indexados, pois nomes de propriedades também podem conter arquivos privados, URLs ou credenciais. A estrutura e a correlação são preservadas, mas seus valores originais não são um mecanismo de replay. Fingerprints vinculam as decisões ao payload efetivo sem gravar esses valores nos metadados. O runtime futuro precisa manter dados transitórios necessários à ação e nunca reconstruir uma operação executável a partir do histórico sanitizado.

Mensagens explícitas do usuário/assistente e cópias informacionais preservam seus textos. Caminhos de apresentação selecionados explicitamente são dados do projeto/raiz; não são usados como autorização para reabrir conteúdo. URLs em fatos de rede perdem credenciais, caminho, query e fragmento antes de persistir.

## Reinício, estados incertos e exclusão

- Reabrir marca execuções/ações não concluídas como interrompidas, sem executar efeitos externos ou inventar resultado de ferramenta.
- Um início que já havia sido reservado permanece incerto; ele não se torna novamente executável só por reiniciar.
- A presença de escritor incerto bloqueia nova aquisição e mudança de contexto naquela conversa. Resolver a condição depende das provas/lifecycle de FUI-014, não de marcar um booleano de interface.
- Excluir conversa remove seus registros, decisões, fatos e cópias de destino; cópias que outras conversas possuem dela permanecem independentes.
- A exclusão é recusada enquanto houver execução ativa ou claim de resultado incerto. A checagem e a cascata estão na mesma transação IMMEDIATE, sem apagar a barreira persistente para contornar o bloqueio.
- Claims são apenas reserva persistida de uma ação: não comprovam que o efeito ocorreu e não substituem autorização/capacidade de produção.

Erros do SQLite preservam classificação e código estendido numérico seguros. Mensagem SQL bruta, argumentos, caminhos e conteúdo de registros não são copiados para o erro público. As provas de reabertura tratam interrupção do processo e estados persistidos; não simulam queda de energia ou falha física de disco.

## Recriação sem migração

O esquema do agente é aplicado no mesmo SQLite usado pela aplicação. Seu conteúdo integra o stamp já existente do banco. Como autorizado, não há migração de dados anteriores: alteração incompatível do esquema provoca a estratégia existente de recriação na próxima abertura real da aplicação.

**O aplicativo Tauri não foi aberto para executar esse fluxo sobre dados do usuário.** Os testes de persistência usam somente bancos em memória ou arquivos temporários próprios. Preparar/compilar os módulos não abre o banco real da aplicação.

## Recursos: identidade não é autorização de conteúdo

- `RootHandle::enroll` é uma entrada interna para raiz selecionada explicitamente por um chamador confiável; não deve receber caminhos arbitrários do modelo como concessão.
- A identidade combina UUID do volume, inode/file ID, geração e criação do diretório. A autorização viva continua dependendo do handle/contexto atual, nunca só da chave serializada.
- Aliases selecionados resolvem para o alvo real; alterar o symlink original não concede seu novo alvo.
- Substituição da raiz, revogação, falta de acesso e indisponibilidade retornam erros tipados com errno preservado, sem retornar um resultado vazio supostamente autorizado.
- `relation` compara objetos e ancestralidade de descritores. Seu resultado é um snapshot; não congela o filesystem nem autoriza efeitos posteriores.
- `RelativePath` rejeita absolutos, `..`, NUL/controles e namespaces ambíguos. Isso não é apresentado como proteção suficiente de I/O por si só.
- `read` e `create_new` retornam `StrictContainmentUnavailable` após validar a raiz, sem abrir o filho ou produzir efeitos. `strict_io_available()` é falso.
- Não existe fallback com `realpath` seguido de leitura comum, flags removidas após erro, ou descritor que continue concedendo leitura por objeto fixado.

**FUI-008 permanece parcial/bloqueado para leitura/criação positiva com contenção estrita.** Identidade, comparação, erros e a interface fail-closed estão implementados; a validação do pai efetivo e identidade de um arquivo realmente criado não está aprovada, pois criação continua indisponível. Esse impedimento não será eliminado dos critérios do ticket.

## Provas e limites

Os testes novos usam as interfaces acordadas, sem consultar conexão SQL privada para provar resultados. Há testes focais de reabertura, idempotência, concorrência de claims, snapshots, escopo de fatos, identidade/aliases, relações, revogação e operações bloqueadas.

A pesquisa nativa e seus limites estão em [reports/resource-access-research.md](reports/resource-access-research.md). A existência de `O_RESOLVE_BENEATH` no host não comprova contenção por localização durante toda a vida de um descritor.

Os resultados completos e a revisão estão em [VERIFICATION.md](VERIFICATION.md): 167 testes Rust aprovados (28 novos nesta onda; 8 previamente ignorados), 21 testes da Onda 1 e 12 smoke tests aprovados. O patch preparado compila e passa Clippy com `-D warnings` isolado da alteração anterior do usuário. Não foi iniciada a Onda 4 nem habilitada uma ferramenta do agente.
