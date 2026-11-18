# Onda 4 — Organização e política central

FUI-009 e FUI-011 implementados por agentes em paralelo, com integração central dos arquivos compartilhados. Foco macOS, validação de arquivos por operação e distribuição adiada, conforme decisões atuais do usuário.

## Divisão de trabalho

- Agente de projetos: operações de `AgentStore`, testes de cadastro/listagem/rename/conversas/relocalização e disponibilidades da raiz.
- Agente de política: avaliador puro e testes determinísticos de permitir/solicitar/bloquear.
- Integração: comandos nativos, seleção explícita de pasta, projeções IPC, cliente TypeScript, validação e revisão.

Os agentes não alteraram os mesmos módulos de implementação. Registro dos módulos e alterações no `lib.rs` ficaram sob um único responsável. Não houve commit automático por subagente nem inclusão das alterações anteriores do checkout.

## FUI-009 — Projetos e conversas

Implementação em `src-tauri/src/foundry/db/agent/projects.rs`.

- Cadastro a partir de `RootHandle` selecionado explicitamente, identidade única e nome inicial derivado da pasta quando não informado.
- Listagem, consulta e renomeação de metadados; renomear o projeto não renomeia sua pasta.
- Conversas com projeto opcional, contexto independente e modo inicial `Review`.
- Listagem por projeto/sem projeto, incluindo conversas legadas sem contexto, filtros de arquivamento e ordenação consistente com a listagem existente.
- Disponibilidade `available`, `missing`, `changed`, `denied` ou `unsupported`, sem trocar a identidade vinculada silenciosamente.
- Histórico e snapshots continuam consultáveis quando a pasta desaparece ou é substituída.
- Relocalização explícita e atômica, conflito de pasta já usada por outro projeto, revisão contextual incrementada e decisões antigas invalidadas quando o vínculo muda.
- Operações de vínculo recusam execuções ativas/incertas e mensagens legadas em streaming associadas ao projeto.
- Seleção de pasta obsoleta é rejeitada pela comparação transacional de geração/identidade/caminho do vínculo esperado, inclusive após A→B→A; renomear apenas o rótulo não muda a geração da raiz.

O caminho persistido é usado para reencontrar a raiz e comparar sua identidade; não é uma autorização para aceitar um alvo diferente. Caminhos não representáveis sem perda são recusados em vez de convertidos silenciosamente.

## Comandos nativos e cliente

Implementação em `src-tauri/src/foundry/project_commands.rs`, registro em `src-tauri/src/lib.rs`, cliente em `src/lib/projects.ts`.

| Comando | Comportamento |
|---|---|
| `projects_pick_create` | Abre seletor nativo e cadastra a pasta escolhida; cancelar retorna `null` |
| `projects_list` / `projects_get` | Retornam metadados e disponibilidade |
| `projects_rename` | Altera somente o nome do projeto no banco |
| `projects_pick_relocate` | Captura vínculo esperado, abre seletor e relocaliza com checagem de obsolescência |
| `projects_create_conversation` | Cria conversa com projeto opcional |
| `projects_list_conversations` | Consulta conversas do projeto ou somente as sem projeto |

Cadastro e relocalização não recebem um caminho arbitrário do frontend. O caminho vem do callback do diálogo nativo; URI de rede, campos de URI descartáveis e caminhos relativos são recusados. Cancelamento não produz uma raiz nem um projeto. Os DTOs usam camelCase, expõem `bindingRevision` para identificar o snapshot e não expõem a chave interna da raiz como se fosse uma credencial. Disponibilidade e caminho são derivados do mesmo registro, sem misturar vínculos de momentos diferentes.

Erros do IPC preservam códigos de domínio distintos, como `ProjectConflict`, `ProjectStaleContext` e `ProjectBusy`, no formato padrão `code/messageKey/message/detail`; o cliente não precisa interpretar texto em inglês para escolher recuperação. Diagnósticos do banco mantêm apenas classe/código seguros.

Não foi construída a navegação de projetos nesta onda; sua integração visual pertence a FUI-027. O diálogo nativo foi integrado e compilado, mas não houve abertura da aplicação real nem interação humana automatizada com esse diálogo nos testes.

## FUI-011 — Política

Implementação em `src-tauri/src/foundry/policy/`:

`evaluate(&Action, &TrustedFacts) -> Decision`

O resultado é `Allow`, `Ask(Vec<Prompt>)` ou `Deny(Denial)`, sem executar qualquer efeito.

- As dimensões de operação, recursos, contexto/revisão, modo, destruição, revisão de diferenças e rede são compostas separadamente.
- As solicitações informam o requisito pendente e se a decisão pode ser lembrada naquela conversa.
- Ações destrutivas e acesso externo a projetos exigem decisões vinculadas à ação, nunca lembradas.
- Concessões de comandos correspondem à operação estruturada completa, não a prefixos de strings; rede é autorização separada.
- Falta de implementação, evidência, validação de recursos, isolamento ou auditoria bloqueia a ação.
- `ReadEvidence` explicita leituras de recursos independentemente do nome da ferramenta, incluindo comandos mutantes e fontes de upload. Leituras não comprovadas ou proibidas pela configuração global são bloqueadas; isolamento/aprovação de comando não as libera.
- Revisão pendente não é aprovada por mudar para modo automático.
- Origem em modelo, arquivo, instrução aprovada ou conversa anexada não produz direitos.

Fatos, aprovações e concessões confiáveis não têm desserialização de payloads do frontend. O módulo não foi exposto como um comando IPC que aceite flags de aprovação. O resolver persistente, consumo de decisões, revalidação no despacho e executor pertencem aos tickets posteriores.

As capacidades positivas dos testes são fatos sintéticos de política; não aprovam o executor de comandos nem um ambiente de produção por si só. Também não substituem o mediador de rede ou a verificação de origem do endpoint de inferência.

## Verificação

As duas interfaces públicas foram confirmadas para TDD, e o usuário aprovou revisão contra `98d0db1`, apenas com os trechos desta onda. Os testes usam bancos/diretórios temporários e cenários puros. Nenhum modelo real, comando do agente ou banco real da aplicação foi iniciado para os testes.

Resultados, limites e revisão ficam em [VERIFICATION.md](VERIFICATION.md). Não foi iniciada a Onda 5 nem habilitado um ciclo de ferramentas no chat.
