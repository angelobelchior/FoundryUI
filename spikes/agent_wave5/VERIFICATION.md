# Verificação da Onda 5

## Recorte

- Base aprovada: `55011ee`, somente módulos e trechos de integração desta onda.
- Interfaces confirmadas: ciclo de contexto com retenção e barreiras; verificação da origem local e revalidação antes do envio.
- Implementação por agentes independentes; arquivos compartilhados, confirmações nativas e transporte integrados centralmente.
- Alterações anteriores do checkout permanecem fora do commit. Sem push.
- macOS; distribuição, assinatura e outras plataformas adiadas.

## Evidências durante a implementação

- A barreira pública da geração teve vermelho por API ausente e verde: mudança bloqueada durante geração ativa, inclusive após solicitar cancelamento, e liberada apenas após seu término.
- A primeira implementação de limpeza por nome teve testes positivos de contenção, mas a revisão encontrou uma corrida entre validação e `unlinkat`. Esses testes não são prova da garantia estrita e aquela implementação foi substituída após confirmação explícita do usuário.
- O armazenamento autorizado usa preparação em memória e bytes no SQLite. Os testes atuais exercitam retenção, cópias independentes, ausência de operações sobre arquivos de origem e exclusão transacional dos metadados/bytes.

## Limites de prova

Os testes usam apenas fixtures e diretórios temporários. Um listener controlado comprova o adaptador nativo nas condições exercitadas; não comprova por si só compatibilidade de uma instalação real do Foundry. Testes sintéticos não são apresentados como comprovação de runtime real.

Os diálogos nativos são compilados e integrados, mas interação com janelas reais precisa ser distinguida de testes controlados. Nenhum banco real da aplicação deve ser aberto para validar o esquema. Não existe migração de compatibilidade nesta versão; alterações de esquema seguem a política de recriação autorizada.

A prova de conexão usa o proprietário real do socket estabelecido, não apenas uma conferência do listener antes de conectar. Ela não é autenticação criptográfica nem proteção contra um runtime/OS comprometido. Fixtures de endpoint/discovery sintéticos são identificadas separadamente das verificações nativas positivas. A instalação real do Foundry não foi iniciada para esta validação e nenhum modelo foi baixado.

As cópias de anexo no banco têm exclusão lógica transacional. Não se promete apagamento forense de blocos livres, journals ou backups externos. Arquivos de instalações anteriores não são migrados nem apagados automaticamente.

## Standards

Na primeira revisão independente, foram encontrados dois P2 de correção:

1. A inspeção nativa síncrona podia bloquear workers do Tokio e atrasar timeout/cancelamento. A preparação e o despacho agora deslocam esse trabalho para o pool de bloqueio; a prova é revalidada depois da serialização, e a inspeção não abre TCP. Os testes públicos de deadline/cancelamento exercitam o pool ocupado e verificam ausência de conexão.
2. Uma observação com `running: false` e PID/URL preservados podia deixar a prova anterior utilizável. A revisão considera a transição de execução; a observação parada não publica endpoint ativo. Um runtime nativo de teste permaneceu escutando para demonstrar vermelho → verde pela interface pública.

Revalidação independente concluída: **0 achados remanescentes de correção, 0 violações documentadas de estilo e 0 achados de heurísticas**. A revisão foi de leitura; não substitui os testes executados pelo integrador.

A revisão da adaptação transacional também encontrou uma reinserção antiga de anexos durante edição de mensagem que preservava apenas os metadados. Ela foi corrigida: o fluxo captura metadados/bytes do registro correto antes da exclusão e os reinsere com IDs novos na mesma transação. Metadados mutáveis recebidos no DTO não são usados como fonte de verdade. Quatro regressões públicas passaram no checkout, incluindo edição/reabertura/leitura e recusa de referências inválidas sem apagar o original.

## Spec

Na primeira revisão independente, foram encontrados dois P1 e um P2:

1. Uma descoberta iniciada antes de uma invalidação podia adotar a revisão nova ao retornar. A revisão é capturada antes da descoberta, com a trava do estado, e a adoção é condicional na mesma trava. Uma resposta antiga não restaura o endpoint após parada. Há regressão pública com uma resposta de CLI retida até a invalidação.
2. `fstatat` seguido de `unlinkat` não fixa a entrada do arquivo entre conferência e exclusão. O usuário confirmou manter a garantia estrita. A implementação por arquivo foi substituída pelo armazenamento transacional de anexos.
3. Limpeza física posterior ao commit podia falhar ou ser perdida num reinício, e os recibos retinham caminhos. Agora os bytes pertencem a registros do SQLite e acompanham a exclusão na mesma transação; recibos guardam apenas identificadores/revisões necessários à repetição idempotente.

Revalidação independente concluída: **0 P1 e 0 P2 remanescentes**. A autorização explícita de armazenamento transacional foi considerada, sem reinterpretar o antigo `unlinkat` como exclusão segura. A revisão foi de leitura, não uma execução independente da suíte.

Ambos os eixos identificaram o mesmo P1 adicional no fluxo de edição de mensagens com imagem, após a mudança de armazenamento. Ele foi corrigido e revalidado nos dois eixos. Os testes cobrem editar, reabrir e ler a imagem preservada, além de recusar anexos de outra mensagem/conversa sem apagar o original.

## Validação final

| Verificação | Resultado |
|---|---|
| Suíte Rust completa no checkout de trabalho | **256 unitários + 25 de integração aprovados**; 0 falhas; 8 previamente ignorados |
| Suíte Rust completa do patch preparado sobre `git archive 55011ee` | **250 unitários + 25 de integração aprovados**; 0 falhas; 8 previamente ignorados |
| `cargo clippy --offline --lib -- -D warnings` no patch isolado | Aprovado |
| `pnpm typecheck` no patch isolado e no checkout | Aprovado |
| `pnpm test:smoke` no checkout | Build e **12 smoke tests** aprovados |
| Regressões dos spikes, `WAVE1_SANDBOX_BACKENDS=macos python3 -m unittest discover -s tests -v` | **21 aprovadas**, sem sandbox pulado |
| Rustfmt dos arquivos alterados e `git diff --cached --check` | Aprovados |

A diferença de seis testes unitários entre checkout e patch isolado vem de alterações anteriores do usuário mantidas fora do commit. Os resultados não são somados: o patch isolado tem **275 testes Rust aprovados**, enquanto o checkout com o trabalho anterior tem **281**.

A primeira compilação isolada identificou um inicializador antigo de `ServerState` incompleto no teste de snapshot. O patch preparado foi ajustado com os campos padrão, sem incluir a alteração equivalente que já existia no checkout do usuário. A suíte isolada acima foi executada depois dessa correção.

Entre os casos focais estão 15 de ciclo de contexto, 17 de preparação de anexos em memória, persistência de anexos/reinício/limite/cópias independentes, quatro regressões de edição com imagem, cinco casos do adaptador de confirmação, 13 de origem/transporte local e duas regressões públicas de descoberta do servidor. As contagens focais já estão incluídas nas suítes completas.

Nenhum teste ignorado foi apresentado como aprovado. Os testes nativos positivos usam executáveis/listeners controlados; um helper temporário de servidor é compilado por `/usr/bin/cc`. Não foi iniciado o Foundry real nem carregado/baixado modelo, e os diálogos reais não foram exercitados interativamente.
