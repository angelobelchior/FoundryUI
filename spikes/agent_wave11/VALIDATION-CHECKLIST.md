# Lista de validações pendentes

Esta lista cobre o que **não foi validado** porque exige rodar o app, um modelo
local real e/ou testes adversários. Ordenada por prioridade/risco. As features
de código estão implementadas; o que falta é **prova** e **revisão manual**.

Legenda: 🔴 bloqueante de segurança · 🟡 funcional · 🟢 cosmética/finalização.

---

## 1. Comandos isolados (FUI-016/026) — 🔴 MAIOR PRIORIDADE

Esta é a área menos provada. `run_command` usa `sandbox-exec` no macOS.

### 1.1 Pré-requisito de ambiente
- [ ] Confirmar que `/usr/bin/sandbox-exec` existe na máquina alvo.
  - ⚠️ `sandbox-exec` está **deprecated** e foi **removido no macOS 15+ (Sequoia)**. Se não existir, o comando retorna `capability_blocked` (esperado) e o FUI-016 precisa de outro mecanismo (ex.: App Sandbox com assinatura, ou seatbelt via `posix_spawn`).

### 1.2 Fluxo funcional (com modelo real carregado)
- [ ] Comando **read-only** (`/bin/echo oi`, `git status`) → retorna a saída, **sem** pedir revisão de diff.
- [ ] Comando **mutante** (`echo x > arquivo.txt`) no modo **revisão** → mostra o diff, **não** toca a pasta real até aprovar; ao aprovar, aplica.
- [ ] Comando **mutante** no modo **automático** → aplica direto.
- [ ] Comando que **exclui** arquivo → exige **confirmação destrutiva**, mesmo em automático.
- [ ] Comando que **não muda nada** → efeito `ReadOnly`, sem diff.
- [ ] Timeout: comando que trava (`sleep 999`) → morto dentro do limite, `status: timed_out`.
- [ ] Saída gigante → truncada, sem travar o app.

### 1.3 Escapes de sandbox (reproduzir o spike da onda 1)
- [ ] Ler arquivo **irmão** (fora da raiz) → negado.
- [ ] Escrever arquivo **irmão** → negado; conteúdo anterior preservado.
- [ ] Symlink criado antes/durante a execução para alvo externo → negado.
- [ ] Rede: listener TCP de loopback alcançável → **não** recebe conexão.
- [ ] Listener Unix socket dentro da raiz → **não** recebe conexão.
- [ ] Variável de ambiente de credencial herdada do pai → **ausente** no filho.

### 1.4 🔴 Lacuna conhecida de lifecycle (não resolvida)
- [ ] Reproduzir: comando que gera um **descendente `setsid()`** e tenta escrever depois do timeout.
  - **Esperado atual:** o descendente **escapa** do `SIGKILL` do grupo e escreve depois.
  - **Decisão sua:** aceitar como limitação documentada (comando `capability_blocked` de novo) **ou** implementar um mecanismo de encerramento que capture a árvore toda (ex.: App Sandbox + `proc_track`, ou matar via `pgrep`/`proc_listchildpids` com retry).
  - ❗ Enquanto essa prova não passar, `cleanup_verified` continua `false`.

---

## 2. Interface — rodar o app e revisar (FUI-028/030/031/032) — 🟡

Necessário: iniciar o app (`pnpm tauri dev` ou build), carregar um modelo local
com `supportsToolCalling`.

### 2.1 Permissões/instruções (FUI-028)
- [ ] Painel "Permissões e instruções": listar concessões, revogar, "conceder pasta" só em conversa sem projeto.
- [ ] Botão **"Aprovar e lembrar"** aparece **só** quando todos os requisitos são `rememberEligible` (não aparece para acesso externo/destrutivo).
- [ ] Aprovar/negar arquivo de instrução mostra preview + versão; aviso "não concede permissões" visível.
- [ ] Decisão expirada/cancelada/invalidada **não** reabre a execução silenciosamente.

### 2.2 Contexto/anexos (FUI-030)
- [ ] Painel "Contexto usado" mostra fontes (original/resumo), omissões e motivo; aviso "limite conservador" quando não há metadados de capacidade.
- [ ] Anexar conversa (inclusive de outro projeto) → aviso de cruzamento de projeto + cópia independente; excluir a origem não quebra a cópia.
- [ ] Remover anexo remove a cópia.

### 2.3 Alterações/desfazer (FUI-031)
- [ ] Diff por arquivo (criado/editado/excluído) visível.
- [ ] "Desfazer" disponível **só** quando há proteção Git real; arquivo sem Git → aviso, sem botão falso.
- [ ] Desfazer criação exige confirmação; desfazer após edição posterior do usuário → **conflito**, sem perda.
- [ ] Sem Git: edição autorizada funciona, mas sem promessa de desfazer.

### 2.4 Capturas e idiomas
- [ ] Capturas em **1440×900** e **960×640**, nos 3 idiomas (**en, pt-BR, es**).
- [ ] Textos longos, alvos múltiplos, estados vazio/duplicado/indisponível/ocupado distintos.

---

## 3. Acessibilidade e localização (FUI-033) — 🟡

- [ ] Navegação **por teclado** em todos os modais/painéis (Tab/Esc/foco retornando ao trigger).
- [ ] Leitor de tela: decisões, alvos, estados e diffs compreensíveis (`role`, `aria-*`, `role="status"`/`"alert"`).
- [ ] Contraste e estados de foco calculados (`projectControlFocus`).
- [ ] T37: fluxo completo de decisão + diferenças via teclado/leitor.

---

## 4. Integração e resiliência (FUI-035) — 🟡

- [ ] Ciclo completo com modelo local real: propor ler/editar/criar → aprovar → aplicar → desfazer.
- [ ] Streaming de chamadas de ferramenta e continuação.
- [ ] **Cancelar no meio** de um efeito → estado interrompido/incerto; sem repetição automática (T28/T30).
- [ ] **Dois agentes no mesmo projeto** → um executor; espera/interrupção explícita (T26).
- [ ] **Queda do app** no meio de efeito → reabre e detecta execução interrompida.
- [ ] Modelo **sem** ferramentas → chat disponível, sem execução de texto livre (T25).

---

## 5. Git e recuperação (FUI-024/025/031) — 🟡

- [ ] Repositório real: editar arquivo **já modificado** pelo usuário → desfazer preserva o conteúdo **anterior à ação**, não o último commit (T32).
- [ ] Arquivo **não rastreado/ignorado** → identificado como não recuperável **antes** de alterar (T33).
- [ ] Desfazer **após** edição posterior → conflito, sem apagar o trabalho novo (T34).
- [ ] Nenhum `reset`/`stash`/`clean`/commit na branch é executado para recuperar.

---

## 6. Rede (FUI-017/032) — 🟡

- [ ] Requisição HTTP mediada: redirecionamento recusado, cada destino reavaliado.
- [ ] URL com senha/token → segredo não aparece em logs/UI/erros (T15).
- [ ] Acesso opaco (não auditável) → operação indisponível.
- [ ] Log por conversa sanitizado, excluído junto com a conversa.

---

## 7. Gate de entrega (FUI-036) — 🟢

- [ ] Revisar `spikes/agent_wave11/DELIVERY-GATE.md` e marcar cada T01–T37 após a validação real (nenhum teste não-executado como "aprovado").
- [ ] **Push** dos ~30 commits de `main` para o origin.
- [ ] Decidir/registrar a resolução da lacuna de lifecycle (item 1.4) antes de declarar comandos como suportados.

---

## Não fazer (restrições do processo)
- ❌ Não iniciar servidores/watchers durante a implementação (você pode rodar na sua validação).
- ❌ Não apagar pastas de projeto/arquivos de usuário.
- ❌ Windows/Linux continuam fora do escopo (FUI-003/004 diferidos).
