# Revisão da Onda 1 macOS

## Recorte confirmado

- Base: `50633d55007385753d213968e4af27b7f1791de6`.
- Comparação WIP aprovada pelo usuário: `git diff --cached 50633d55007385753d213968e4af27b7f1791de6 -- spikes/agent_wave1`.
- Somente os arquivos novos da onda; alterações anteriores do checkout foram excluídas do recorte.
- Especificações de origem: FUI-001, FUI-002, FUI-005 e a decisão posterior de implementar apenas macOS.
- Dois revisores independentes executaram os eixos Standards e Spec em paralelo, com nova verificação após correções.
- Não havia AGENTS.md/CLAUDE.md/CONTEXT.md ou convenção Python específica aplicável no levantamento. O baseline de smells da skill foi aplicado como julgamento, não como norma rígida.

## Standards

Revisão inicial: **0 violações documentadas e 0 smells acionáveis**. A pequena duplicação de fixtures e a complexidade do parser não justificaram refatoração especulativa deste protótipo. Nenhum comentário de código foi introduzido.

Observação adicional de correção, P2: o JSON válido `1e999` era convertido em `float('inf')` pelo Python e passava na validação de argumento `float`, apesar da rejeição de `NaN`/`Infinity` explícitos.

Resolução: `finite_float` verifica finitude e é usado nos dois pontos de decodificação JSON. `test_numeric_exponent_overflow_cannot_return_nonfinite_arguments` reproduziu a falha para expoentes positivos e negativos antes da correção e passou depois.

Re-revisão: achado resolvido; nenhum bloqueio novo identificado.

**Resultado final Standards: 0 violações, 0 smells acionáveis e 0 observações de correção pendentes.**

## Spec

Revisão inicial: **1 achado P2**. Um envelope SSE `{"error": ...}` depois de uma chamada aparentemente concluída era ignorado porque não possuía `choices`; o parser podia devolver a chamada anterior como válida. Isso contrariava o comportamento de falha controlada exigido por FUI-001 e RUN-03.

Resolução: o parser rejeita envelopes de erro explícitos e formatos inválidos, mantendo suporte a frames legítimos de usage-only. `test_provider_error_or_invalid_envelope_invalidates_prior_calls` falhou antes da correção e passou depois, sem expor o texto privado do erro do provedor.

Re-revisão: achado original resolvido; os 8 testes focais de protocolo passaram, incluindo CRLF, interleaving e usage-only. Nenhum novo bloqueio identificado.

**Resultado final Spec: 0 achados pendentes.**

## Verificação posterior

A suíte completa da onda passou com 21 testes e sandbox macOS explicitamente habilitado. O resultado negativo de descendentes continua documentado como insuficiência do candidato, não como sucesso de segurança. Nenhum módulo de produção foi habilitado para contornar esse resultado.

O commit desta entrega não inclui o diretório externo `FoundryUI-Specs`, que permanece fora do repositório por decisão do usuário; os estados e decisões lá registrados foram atualizados separadamente.
