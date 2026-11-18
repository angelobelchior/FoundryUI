# Onda 1 — Resultados macOS

Data: 2026-11-18. Escopo atualizado pelo usuário durante a implementação: focar somente macOS; Windows/Linux adiados. As interfaces de teste foram confirmadas antes da escrita dos testes.

## Ambiente observado

| Item | Observação |
|---|---|
| Host | macOS 27.3, Darwin 27.3.0, ARM64 |
| Python | 3.15.2; harness sem pacotes de terceiros |
| Foundry CLI | 0.13.0 |
| Runtime informado pelo daemon | ONNX Runtime 1.31.0; ORT GenAI 0.18.0 |
| Modelo cached e carregado | `qwen3-14b-generic-gpu:2` |
| Capacidade declarada | `supportsToolCalling: true`, na resposta do catálogo local |
| Capacidade observada | Uma chamada estruturada, resultado correlacionado e continuação, com `/no_think` |

O daemon estava parado no levantamento inicial. Após autorização do usuário para iniciá-lo, `foundry model list --cached --output json` iniciou o daemon implicitamente. Não foi solicitado download de modelo; a variante Qwen já estava em cache e foi carregada explicitamente. O runtime informou reutilização de um bundle WebGPU verificado existente.

O startup também informou inicialização de telemetria e acesso a metadados do catálogo. Este experimento não demonstra ausência de rede do próprio Foundry/SDK. Nenhum conteúdo de pasta de projeto foi enviado ao modelo: apenas instruções e valores de uma fixture fixa em memória.

## FUI-001 — Protocolo e modelo

### Resultado real

Comando do harness, com a porta local efetiva daquela execução:

```sh
python3 -m wave1 model --endpoint http://127.0.0.1:51592 --model qwen3-14b-generic-gpu:2 --no-think
```

Saída observada em execuções completas, incluindo repetição após as correções da revisão:

```json
{
  "status": "cycle_observed",
  "model": "qwen3-14b-generic-gpu:2",
  "tool_calls": 1,
  "reason": null,
  "product_ready": false,
  "thinking_disabled": true
}
```

A ferramenta `lookup_fixture` recebe exclusivamente `{"key":"sample"}`. O harness retorna `fixture-value` como mensagem `tool`, usando o mesmo `tool_call_id`. A continuação deve devolver exatamente esse valor, sem nova chamada. Não há dispatcher para arquivos ou terminal.

A repetição final usou um daemon iniciado explicitamente com porta atribuída pelo sistema e idle timeout de cinco minutos, em `127.0.0.1:52896`. Após a prova, sua identidade foi conferida e o daemon pertencente ao experimento foi encerrado com sucesso. Nenhum daemon de outra identidade foi encerrado pelo cleanup.

O primeiro experimento sem `/no_think` não comprovou o ciclo. Uma inspeção limitada mostrou resposta HTTP 200 e longa geração de `reasoning_content`; o resultado foi mantido como não comprovado, sem executar texto que imitasse ferramenta. Com o soft switch explícito, foi observada uma chamada em `delta.tool_calls` e término `tool_calls`.

### Contrato para FUI-013

- Processar `data:` SSE, CRLF e comentários de keepalive; preservar bytes UTF-8 entre fragmentos.
- Montar chamadas por índice e correlacionar ID, nome e argumentos. Campos opcionais nulos em deltas de continuação não são uma ferramenta diferente.
- Exigir término compatível e `[DONE]`; validar o lote completo antes de disponibilizar ações.
- Rejeitar ferramenta desconhecida, campos/tipos de argumentos incorretos, IDs repetidos, chaves JSON duplicadas e números JSON não padronizados.
- Rejeitar excesso de stream/chamadas/argumentos e não recuperar um stream que já excedeu o limite.
- Manter conteúdo e raciocínio separados de chamadas estruturadas. Strings Harmony, Markdown ou JSON em texto nunca viram execução.
- Correlacionar resultado com a chamada no histórico. Suporte declarado não substitui a prova com a combinação efetiva.

Limites do spike: 1 MiB de wire SSE, até 16 índices de chamada, 65.536 caracteres de argumentos por chamada, nomes até 128 caracteres e IDs até 256. Não são limites finais de produto. O parser acumula um stream limitado antes de validar; não é uma implementação incremental de interface para produção.

## FUI-002 — Candidato macOS

### O que foi exercitado

`sandbox-exec` com negação por padrão, raiz de fixture autorizada, ambiente explícito reduzido, rede negada e exceções de runtime do sistema.

| Prova | Resultado observado |
|---|---|
| Ler e criar arquivos na fixture | Permitido |
| Ler arquivo irmão fora da raiz | Negado |
| Escrever no arquivo irmão | Negado; conteúdo anterior preservado |
| Ler symlink criado antes da execução para alvo externo | Negado |
| Criar symlink durante a execução e ler alvo externo | Negado |
| Herdar variável sintética de credencial do processo pai | Variável ausente |
| Acessar listener TCP de loopback alcançável pelo processo de controle | Negado; listener não recebeu conexão do candidato |
| Acessar listener Unix dentro da raiz autorizada | Negado; listener não recebeu conexão do candidato |
| Encerrar descendente que chamou `setsid()` | **Insuficiente: o descendente escreveu depois do timeout** |

### Prova negativa de lifecycle

Uma fixture Perl faz fork, abre uma nova sessão e agenda uma escrita finita dentro da raiz temporária. O processo original excede o timeout e seu grupo é encerrado. O descendente de outra sessão escreve `detached-marker` depois disso.

O teste `test_detached_descendant_demonstrates_group_kill_is_not_containment` é verde quando reproduz essa insuficiência e verifica `cleanup_verified: false` e `product_ready: false`. Ele não representa aprovação de cancelamento. Não houve escrita fora da fixture nem criação de processo sem término planejado.

**Conclusão: candidato rejeitado como executor completo de comandos.** Preservar restrições de arquivo/rede não basta para liberar bloqueio de recursos como se toda a árvore tivesse terminado.

### Distribuição e limites

- O mecanismo depende de `/usr/bin/sandbox-exec` e de SBPL privado do sistema. Não há garantia de compatibilidade do perfil com todas as versões macOS suportadas pelo aplicativo.
- O manual local `/usr/share/man/man1/sandbox-exec.1` identifica o comando explicitamente como `DEPRECATED` e recomenda App Sandbox para aplicativos. Isso reforça que a prova não é uma escolha de distribuição aprovada.
- Foram consultados os arquivos locais `/System/Library/Sandbox/Profiles/dyld-support.sb`, `system.sb` e `bsd.sb`. Seus próprios avisos descrevem interface privada, sujeita a mudanças.
- O perfil importa apenas o suporte de dyld necessário para o executável iniciar; não importa o perfil `system.sb` inteiro, que concede acessos adicionais.
- Mesmo a leitura de metadados globais e as exceções de runtime precisam ser inventariadas antes de definir uma política de produção.
- Não foram comprovados todos os casos de hardlinks, substituição concorrente de raiz, escape por brokers, macOS 15, perda do launcher ou recursos de produção.
- Não houve alteração de permissões de arquivos reais, configuração de segurança do host, assinatura do bundle ou dependências do aplicativo.

## FUI-005 — Rede mediada

Provas com servidor HTTP de loopback temporário, encerrado ao fim de cada teste:

- GET autorizado retorna corpo e eventos correlacionados.
- Cada redirecionamento produz evento e exige nova decisão antes do próximo transporte.
- Negação, resultado não booleano e exceção do autorizador bloqueiam o envio.
- Protocolos não suportados, URL malformada, controles e credenciais embutidas são bloqueados.
- Limite de corpo descarta resposta excedente; limite de redirecionamentos impede loop ilimitado.
- Eventos ocultam conservadoramente todo caminho não raiz e os valores de query; erros não copiam a exceção bruta ou a URL secreta.

O contrato usa estados `requested`, `blocked`, `attempted`, `redirected`, `completed` e `failed`. Um pedido ou uma tentativa não é anunciado como acesso concluído. A implementação posterior deverá persistir os eventos por conversa e implementar cancelamento/deadline global, retenção e políticas completas de destinos.

O cliente usa conexões diretas de `http.client`, sem proxy herdado nem redirecionamento automático. HTTPS usa validação padrão de certificados; essa propriedade não foi confundida com uma auditoria externa completa de TLS nesta onda. HTTP sem TLS é restrito a IP literal de loopback para fixtures.

**Conclusão:** prova de mediação explícita disponível para a família suportada. Comandos opacos não recebem liberação de rede. Isso não audita o tráfego do Foundry, do host ou de aplicativos externos.

## Plataformas adiadas

Windows não foi executado. Antes da alteração de foco houve experiências limitadas em Docker Linux ARM64 com imagem AzureLinux já disponível, sem pull: acesso à fixture montada, falta de rota externa, redução de ambiente e remoção de contêiner com escritor em background. Não houve prova de sockets/brokers nem de distribuição Linux desktop; nenhum suporte Linux foi aprovado.

Após a instrução de focar macOS, o código de execução Docker experimental foi removido do entregável. A pesquisa documental permanece como referência, não como requisito de implementação desta entrega.

## Estado para o próximo gate

| Capacidade | Estado |
|---|---|
| Chamadas estruturadas na combinação Qwen/Foundry descrita | Observada; não habilitada no produto |
| Protocolo malformado/texto livre | Rejeitado nos cenários de teste |
| Mediação de GET e eventos sanitizados | Exercitada em fixtures; não integrada |
| Executor macOS de comandos arbitrários | Bloqueado; candidato de lifecycle reprovado |
| Windows/Linux | Adiados pelo usuário |

## Verificações finais executadas

| Comando | Resultado observado |
|---|---|
| `WAVE1_SANDBOX_BACKENDS=macos python3 -m unittest discover -s tests -v` | 21 aprovados, 0 falhas, sem provas de sandbox puladas |
| `pnpm typecheck` | Aprovado; executado regularmente durante os slices |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml` | Aprovado |
| `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib` | 139 aprovados, 0 falhas, 8 testes previamente ignorados |
| `pnpm test:smoke` | Build aprovado e 12 smoke tests aprovados; preview local limitado ao ciclo dos testes |
| `python3 -m compileall -q wave1 tests` | Sintaxe Python válida; não equivale a typecheck Python |
| Revisão Standards/Spec | Dois achados corrigidos e revalidados; nenhum pendente |

As duas correções da revisão foram overflow numérico para infinito e envelopes explícitos de erro SSE indevidamente ignorados. Ambas têm regressões observadas em vermelho antes da correção e verdes depois. A fixture de usage-only válida permanece aceita.

A conclusão de FUI-002 é uma conclusão investigativa negativa, não uma redução das garantias exigidas pelo produto. Ver o [registro da revisão](review.md).
