# Wave 1 — Provas do agente local

Spikes isolados de FUI-001, FUI-002 e FUI-005. A implementação atual foi limitada a **macOS** por decisão do usuário durante a execução. Windows e Linux permanecem adiados.

**Nada neste diretório habilita ferramentas no chat.** Não há registro de comandos Tauri, alteração do banco ou integração com a interface do aplicativo.

## Resultado principal

- Ciclo estruturado observado no Foundry 0.13.0 com `qwen3-14b-generic-gpu:2`, usando o soft switch explícito `/no_think`.
- Parser rejeita lotes incompletos/malformados, IDs duplicados, tipos indevidos, excesso de limites e texto que imita comandos.
- Rede mediada GET com autorização por destino, redirecionamentos limitados e eventos sanitizados foi exercitada contra fixtures locais reais.
- `sandbox-exec` restringiu os arquivos e sockets das fixtures, mas **foi rejeitado como executor completo**: um descendente que abriu outra sessão escreveu depois do encerramento do grupo original.
- Todos os resultados de protótipos permanecem `product_ready: false`. Uma prova de protocolo não prova segurança de um agente com efeitos reais.

As evidências e os limites estão em [reports/macos-findings.md](reports/macos-findings.md). A [pesquisa documental](reports/windows-linux-research.md) registra alternativas para as plataformas adiadas e não comprova sua execução.

## Por que Python

O harness usa Python 3.10+ e somente sua biblioteca padrão, sem dependências de terceiros, para tornar os experimentos de protocolo, processos e rede reproduzíveis sem alterar a aplicação Rust/React ou instalar pacotes. É código de investigação, não a escolha de runtime de produção.

As interfaces observáveis, confirmadas pelo usuário antes dos testes, são:

| Interface | Operação observada |
|---|---|
| `ToolStream.feed / finish` | Receber bytes SSE e retornar somente chamadas completas e validadas |
| `ProbeRunner.run` | Executar uma fixture com perfil de arquivos/rede, observar resultado e limitações do cancelamento |
| `MediatedClient.fetch` | Solicitar um destino com autorização e produzir resultado/eventos sanitizados |

`probe_local_model` exercita o protocolo em duas chamadas HTTP usando apenas uma ferramenta fixa em memória. A fixture de teste desse fluxo é identificada como sintética; ela não substitui a prova com modelo real.

## Executar testes

Na pasta `spikes/agent_wave1`:

```sh
python3 -m unittest discover -s tests -p test_protocol.py -v
python3 -m unittest discover -s tests -p test_model.py -v
python3 -m unittest discover -s tests -p test_network.py -v
WAVE1_SANDBOX_BACKENDS=macos python3 -m unittest discover -s tests -p test_sandbox.py -v
```

Suíte completa desta onda, incluindo o candidato macOS:

```sh
WAVE1_SANDBOX_BACKENDS=macos python3 -m unittest discover -s tests -v
```

Sem a variável, as provas reais de sandbox são puladas explicitamente. Não registrar esse resultado como prova de isolamento.

Os testes de rede criam listeners de loopback/Unix temporários, encerrados no fim de cada teste. Não iniciam servidor de desenvolvimento nem daemon persistente. O teste negativo de descendente usa um processo finito e escreve somente uma fixture temporária; um teste verde nesse caso significa que a insuficiência foi reproduzida, não que o cancelamento é seguro.

## Provar um modelo já disponível

```sh
python3 -m wave1 environment
python3 -m wave1 model --endpoint http://127.0.0.1:PORTA --model qwen3-14b-generic-gpu:2 --no-think
```

Substituir `PORTA` pelo endpoint atual de um daemon autorizado. A CLI do harness não inicia serviços, baixa modelos, carrega modelos ou procura outro endpoint. Sua saída é JSON e o código de saída é diferente de zero quando o ciclo não é comprovado.

O usuário autorizou iniciar o Foundry nesta sessão; isso não faz o harness iniciar serviços implicitamente. A consulta de catálogo do Foundry observada durante a prova iniciou o daemon automaticamente. O comportamento do CLI/runtime precisa ser tratado separadamente de uma garantia offline do produto.

`--no-think` é uma condição explícita da prova Qwen, não uma correção genérica para todos os modelos. O resultado registra `thinking_disabled`. Sem esse modo, a tentativa inicial não comprovou o ciclo dentro dos limites do experimento.

## Limites de segurança do experimento

- `ProbeRunner` serve apenas para comandos e diretórios de fixture controlados pelo desenvolvedor. Não conectá-lo a entradas de modelo ou a pastas reais de usuário.
- O perfil SBPL e `sandbox-exec` usam interfaces privadas/depreciadas do sistema e exceções mínimas de runtime. O relatório identifica a versão efetivamente exercitada.
- `cleanup_verified` permanece falso no candidato macOS. Encerrar um grupo de processos não comprova que não restam descendentes.
- A captura temporária de saída é truncada na leitura, mas não representa um coletor de produção com limite de disco. Comandos arbitrários não são suportados.
- A mediação implementada cobre apenas GET. HTTP sem TLS só é aceito para IP literal de loopback nas fixtures; HTTPS usa a verificação padrão de certificados.
- Corpos de resposta são dados, não logs; a sanitização cobre metadados dos eventos. O chamador continua responsável pelo tratamento de conteúdo retornado.
- Eventos incluem IDs de conversa/execução/chamada, destino sanitizado, data e resultado. O spike não implementa ainda o armazenamento e a retenção de produção.
- Caminhos e valores de query são ocultados conservadoramente nos eventos. Credenciais em URL são recusadas antes do transporte.
- O timeout de rede é por operação de socket; deadline global, cancelamento e políticas DNS/IP completas pertencem à implementação posterior.

## Organização

| Caminho | Conteúdo |
|---|---|
| `wave1/protocol.py` | Decodificação SSE, montagem e validação atômica de chamadas |
| `wave1/model.py` | Prova local de chamada, resultado correlacionado e continuação |
| `wave1/network.py` | Mediação explícita e eventos sanitizados |
| `wave1/sandbox.py` | Candidato macOS reprovado e resposta indisponível para outros backends |
| `tests/` | Testes nas três interfaces acordadas |
| `reports/` | Resultados, limitações e fontes primárias |

## Verificação desta entrega

- Suíte completa da onda no macOS: **21 testes aprovados**, sem skips de sandbox.
- Aplicação existente: typecheck e build aprovados; **139 testes Rust aprovados**, 8 já ignorados; **12 smoke tests aprovados**.
- Revisão em dois eixos concluída, com duas correções de robustez e regressões observadas em vermelho e verde. Resultado final: nenhum achado pendente em Standards ou Spec. Ver [reports/review.md](reports/review.md).

Esses resultados não promovem o candidato de comandos reprovado. O teste negativo de lifecycle continua sendo evidência de bloqueio.

## Próximo gate

FUI-006 deve consumir os resultados sem promover o candidato de comandos reprovado. Uma implementação segura de lifecycle/isolamento ainda precisa ser escolhida e comprovada antes de habilitar comandos arbitrários. Esta onda não implementa FUI-006 nem os módulos de produção posteriores.
