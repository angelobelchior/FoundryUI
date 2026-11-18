# Continuação — Acesso validado por operação

## Decisão atual

Após a primeira entrega da Onda 3, o usuário pediu foco em implementação e confirmou a opção **“Validar por operação”**. A regra anterior de cessar acesso imediatamente pela movimentação externa foi substituída explicitamente.

- Validar raiz e alvo ao abrir a operação.
- Bloquear absolutos, travessia e symlinks de filhos.
- Trabalhar no mesmo objeto validado até concluir a operação, inclusive se outro aplicativo o mover ou substituir o nome original depois da abertura.
- Manter revogação explícita, permissões por conversa, confirmação destrutiva e autorização de rede.
- Adiar assinatura, App Sandbox, empacotamento e distribuição.

Não há um novo caminho para executar comandos, ignorar políticas ou acessar outros recursos por nome fora da raiz.

## Implementação nativa

A resolução usa `openat` relativo ao descritor da raiz, `O_NOFOLLOW_ANY`, `O_CLOEXEC` e `O_RESOLVE_BENEATH`. A constante Darwin `O_RESOLVE_BENEATH` vem do SDK documentado na [pesquisa](reports/resource-access-research.md); a implementação verifica em runtime que caminhos absolutos e `..` são rejeitados com `ENOTCAPABLE`. Ausência dessas semânticas retorna `OperationContainmentUnavailable`, sem remover a proteção e tentar novamente.

`open_read` permite somente arquivos regulares no volume selecionado. `OpenedRead` não expõe o descritor, não é desserializável e é consumido por `read`. A raiz pode ter seu direito revogado explicitamente antes da entrega dos dados; alteração apenas do nome/localização não troca o objeto já aberto.

A leitura é limitada a no máximo 16 MiB por operação e ao limite menor pedido pelo chamador. Excesso não retorna um prefixo fingindo leitura completa. Há verificação de tamanho/mtime antes e depois para detectar mudanças observadas, sem promessa de snapshot atômico sob um escritor concorrente.

Na criação, o pai é resolvido a partir da raiz e fixado como objeto da operação. O último componente é criado com `O_CREAT | O_EXCL` e modo `0600`, e o descritor efetivo é validado antes da escrita. Se o pai for movido após essa validação, a operação continua nesse mesmo pai; uma nova operação precisa resolver seu caminho novamente a partir da raiz. Isso é parte da semântica por operação, não uma permissão permanente para a localização externa.

A criação não sobrescreve arquivos ou symlinks existentes. Falha de escrita/sincronização depois da criação é `WriteIncomplete` e pode deixar um arquivo parcial; o módulo não apaga por nome um alvo que outro aplicativo possa ter trocado. Propostas, revisão e recuperação continuam pertencendo aos tickets posteriores.

## Escopo e limites

- Exercitado no host macOS ARM64 atual com filesystem local. Não há alegação de distribuição pronta ou suporte provado em todas as versões macOS.
- Volumes não locais ou alvos de outro volume são recusados para conteúdo. A resolução/consulta de metadados do sistema não é apresentada como auditoria de todas as comunicações do host ou de automounts.
- Arquivos especiais são rejeitados sem leitura de corpo; `O_NONBLOCK` evita bloquear ao abrir FIFOs para classificação.
- A aplicação hospedeira continua confiável; estas operações não isolam código nativo arbitrário. O executor de comandos reprovado não foi promovido.
- Nenhuma rota Tauri ou ferramenta de modelo foi registrada. As políticas de conversa deverão ser impostas pelos módulos responsáveis antes de consumir estas operações.

## Testes públicos

Os testes de recursos confirmam leitura/criação reais, criação em pai validado, exclusividade, modo de arquivo, symlinks internos/externos negados, recursos especiais negados, leitura vazia versus erro, limite de bytes, revogação e a leitura do objeto original após mover/substituir seu caminho.

Um pai movido antes de começar a operação não é procurado em sua localização externa. A primeira seleção de raiz por alias continua fixando o alvo real, sem ganhar o alvo de um symlink retargetado posteriormente.

## Verificação e revisão

A mudança mantém as duas seams de persistência/recursos já confirmadas e o recorte de revisão da Onda 3, sem incluir alterações anteriores do usuário.

| Verificação executada | Resultado |
|---|---|
| Testes públicos de recursos macOS | 14 aprovados |
| Suíte Rust completa | 173 aprovados, 0 falhas, 8 testes previamente ignorados |
| Regressões da Onda 1 | 21 aprovadas, sem sandbox pulado |
| `pnpm test:smoke` | Build e 12 smoke tests aprovados |
| `pnpm typecheck` | Aprovado |
| Patch preparado isolado sobre `967e002` | Compila e passa Clippy com `-D warnings`, sem incluir alterações anteriores do usuário |

Os testes da aplicação usam o checkout de trabalho, incluindo alterações anteriores preservadas. A validação isolada comprova que o incremento compila independentemente delas. Não houve nova inferência real, download ou alteração do banco real da aplicação.

### Standards

Uma observação P2 foi corrigida: erro na validação do descritor depois da criação agora é classificado como `WriteIncomplete`, preservando errno. O revisor confirmou a correção e não identificou bloqueio novo. **0 achados remanescentes.**

### Spec

O mesmo caso de efeito parcial foi identificado e corrigido, com teste na interface pública observado vermelho → verde. O teste usa uma pequena biblioteca C temporária para injetar `EIO` em `fstat` apenas no arquivo de fixture, dentro de um processo filho de teste. Confirma que a entrada criada permanece vazia e não é removida por nome. Não adiciona helper de produção, assinatura, entitlement ou mudança de segurança do sistema.

Re-revisão: **0 defeitos remanescentes e nenhum bloqueio de FUI-008 para a fundação macOS no contrato por operação atual**. Isso não equivale a distribuição universal pronta nem à integração de ferramentas no chat.
