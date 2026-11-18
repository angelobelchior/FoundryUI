# Contratos propostos e habilitação no backend

## Status

Direção técnica de FUI-006, derivada das provas da Onda 1 e das especificações aprovadas. Os nomes são conceituais; a implementação Rust pode ajustá-los mantendo invariantes, erros e critérios de aceite.

Não há registro de ferramentas do agente, nova rota Tauri, tabela, worker ou executor de produção nesta entrega. Os protótipos Python não são adapters de produção aprovados.

## 1. Contexto de uma ação

Os módulos que produzem efeitos devem receber um contexto resolvido pelo backend, contendo pelo menos:

| Informação | Invariante |
|---|---|
| Conversa, execução e ação/chamada | IDs correlacionados, com propriedade e estado verificados |
| Revisão contextual | Corresponde ao contexto atual; mover conversa invalida decisões antigas |
| Operação e alvos | Estruturados e delimitados; não inferidos de prefixo textual de shell |
| Identidade e versão do recurso | Verificadas no acesso efetivo, não apenas na montagem do prompt |
| Ambiente relevante | Plataforma/versão/arquitetura, runtime, variante/opções do modelo e identidade do mecanismo exigido |
| Modo e decisões vigentes | Revisão/automático, concessões contextuais, confirmações destrutivas e autorização de rede separada |

O frontend e o modelo podem solicitar ações, mas não construir um contexto confiável apenas enviando esses campos. O backend resolve e valida a referência com seus próprios registros.

## 2. Modelo e protocolo

**Interface conceitual:** iniciar uma geração com histórico estruturado e ferramentas permitidas; observar texto, raciocínio disponibilizado pelo provedor, chamadas validadas e resultado final.

Entradas: contexto da execução, endpoint verificado, modelo/variante/opções, orçamento, histórico com chamadas/resultados e schemas efetivamente permitidos.

Saídas: eventos identificados e ordenados; lote de chamadas completas; motivo de término/erro. Texto/raciocínio e proposta de ferramenta continuam distintos.

Invariantes:

- A flag declarada `supportsToolCalling` não aprova a capacidade; consultar a evidência aplicável e as demais garantias.
- A prova Qwen é condicionada ao ambiente e a `/no_think`; não alterar silenciosamente comportamento de outros modelos para alegar compatibilidade.
- Validar enquadramento SSE, IDs, argumentos, tipos, limites e término antes de despachar o lote.
- Erro explícito do provedor, stream incompleto, overflow numérico ou erro de esquema invalida as chamadas do lote, mesmo se parte delas parecer completa.
- Uso de `[DONE]`, finish reasons e campos opcionais deve seguir o contrato do provedor comprovado, sem converter marcadores de texto em ferramentas.
- Resultados entram no histórico com `tool_call_id` correspondente. Falha/negação não vira aprovação e reconectar não reexecuta a chamada.
- Metadados/tipos não conhecidos recebem incompatibilidade ou erro controlado; chat sem ações continua disponível.

Responsáveis: FUI-013; endpoint em FUI-018; apresentação em FUI-029.

## 3. Isolamento e lifecycle

**Interface conceitual:** consultar garantias efetivas do ambiente, iniciar uma ação autorizada e pedir interrupção com resultado de limpeza verificável.

Entradas: contexto confiável, operação delimitada, recursos permitidos, política de rede, limites de recursos e cancelamento. O módulo encapsula o mecanismo de isolamento, sem expor ao chamador um processo irrestrito como alternativa.

Saídas: identificador opaco da execução isolada; eventos limitados de saída; conclusão com efeitos realizados; resultado de interrupção que distingue escritores encerrados de estado incerto.

Invariantes:

- Garantias abrangem descendentes e caminhos indiretos, inclusive sockets/brokers e ambiente herdado.
- Recurso sem implementação ou prova aplicável retorna indisponível antes de iniciar efeitos.
- Negação de rede não pode depender apenas de variável de proxy; política de arquivo não pode depender só do diretório de trabalho.
- Falhar ao criar o isolamento encerra a tentativa. O comando original não é executado fora dele.
- Encerrar o processo original ou seu grupo não comprova término de descendentes em outras sessões.
- Cancelar exige impedir novos efeitos e verificar a ausência de escritores. Se isso não puder ser comprovado, manter estado incerto e bloquear um novo escritor conflitante.
- Saída deve ter limites durante a coleta, não apenas truncamento ao ler um arquivo temporário já potencialmente grande.
- Em modo de revisão, comandos mutantes precisam preparar diferenças fora da pasta real ou permanecer bloqueados.

**Adapter atual:** `sandbox-exec` + `killpg` está reprovado para esse contrato (CAP-07/CAP-08). Seu uso nas fixtures não autoriza incorporá-lo como executor de produção.

FUI-014/FUI-016 devem escolher e provar um mecanismo que satisfaça o contrato. Esta onda não escolhe uma alternativa não testada apenas para preencher a interface.

## 4. Rede mediada

**Interface conceitual:** solicitar uma operação de rede suportada com autorização contextual e obter resultado limitado mais eventos auditáveis por destino.

Entradas: contexto, URL/operação validada, orçamento, deadline/cancelamento e decisão de autorização resolvida pelo backend. Segredos necessários ao transporte são separados de campos persistíveis.

Saídas: resultado observado; corpo limitado tratado como dado; eventos sanitizados com conversa/execução/chamada, destino, data e estado.

Invariantes:

- Autorizar comando não autoriza rede, e autorizar um destino não autoriza novos destinos de redirecionamento.
- Reavaliar cada salto antes de emitir transporte. Sem decisão válida, registrar bloqueio e não conectar.
- Tentativa, resposta, falha e cancelamento são estados distintos. Nenhum evento inventa entrega confirmada ao servidor.
- Evitar credenciais em URLs e ocultar valores sensíveis antes de persistência, erro, apresentação, cópia ou exportação de metadados.
- Não reenviar credenciais a outro destino automaticamente.
- Protocolos/operações sem controle e auditoria ficam indisponíveis. O caminho de comandos continua sem rede direta.
- Corpos não são logs; sua leitura/uso como contexto tem política e orçamento próprios.
- Falha de auditoria/persistência necessária impede iniciar o próximo transporte, em vez de gerar um acesso silencioso.
- Acesso à ferramenta não libera outro provedor de inferência nem comprova ausência de tráfego do próprio Foundry/SDK.

O protótipo comprova uma parte desse contrato em GET/HTTP loopback. HTTPS completo, deadline global, cancelamento e persistência continuam GAP-04. Responsáveis: FUI-007/FUI-017 e FUI-018 para inferência.

## 5. Habilitação: parte da política, não preferência da interface

**Interface conceitual:** avaliar se as garantias de uma operação estão disponíveis no ambiente efetivo e retornar capacidade elegível ou negação fundamentada.

Essa avaliação deve compor o módulo de autorização do backend, sem duplicar regras em cada botão ou no parser do modelo. O resultado de capacidade elegível ainda não é permissão da conversa.

Ordem de decisão:

1. Resolver contexto e operação pelo backend.
2. Exigir implementação de produção e garantias de capacidade aplicáveis.
3. Avaliar as permissões e confirmações contextuais da ação.
4. Registrar intenção/decisão e adquirir os recursos necessários.
5. Revalidar contexto, alvos, versões e garantias imediatamente antes do efeito.
6. Despachar somente pelo módulo controlado; persistir o resultado observado.

Se a dependência de uma operação estiver reprovada ou não comprovada, a operação composta é negada. CAP-05/CAP-06 não compensam CAP-07; CAP-02 não compensa GAP-02; uma aprovação do usuário não compensa CAP-11.

### Origem e validade das garantias

- Registro mantido pelo backend e associado a código/mecanismo de produção e evidência aplicável; nenhum arquivo de projeto é fonte de autoridade.
- Mudança de ambiente ou política invalida as garantias pertinentes até reavaliação. Evidência histórica permanece histórica.
- Uma combinação fora da cobertura recebe “não comprovado”; ausência, erro de consulta ou versão desconhecida não significam permissão.
- A matriz Markdown é inventário técnico. Não deve ser interpretada como allowlist executável nem sobrescrita por dados enviados pelo frontend/modelo.
- Implementar bypass de “continuar mesmo assim” para uma garantia obrigatória contrariaria o contrato, inclusive em modo automático.

FUI-011 implementa a composição da política; FUI-012, decisões e revogação; FUI-013/FUI-016/FUI-017 devem revalidar no ponto de despacho. FUI-007 deve preservar a vinculação entre contexto, ação e decisões sem reutilizar aprovações para outro alvo.

## 6. Diagnósticos propostos

Os códigos abaixo são nomes conceituais, a estabilizar na implementação; não prometem IPC já existente.

| Código | Quando usar | Efeito esperado |
|---|---|---|
| `production_implementation_missing` | Existe apenas protótipo ou o módulo ainda não foi implementado | Bloquear ação, mantendo histórico/chat disponíveis |
| `capability_unproven` | Ambiente/modelo/opções fora da cobertura positiva | Bloquear a capacidade; não iniciar uma prova com efeitos sem autorização |
| `capability_rejected` | Há contraexemplo aplicável ao contrato exigido | Bloquear sem fallback; indicar a garantia reprovada |
| `platform_deferred` | Windows/Linux no recorte atual | Informar indisponibilidade, sem sugerir suporte por pesquisa documental |
| `model_tools_incompatible` | Protocolo/modelo não comprovado como compatível | Manter conversa sem ações e permitir escolher modelo compatível |
| `endpoint_provenance_unverified` | Origem do endpoint não confirmada para contexto local | Não enviar arquivos ou snapshots silenciosamente |
| `process_cleanup_uncertain` | Cancelamento não comprova ausência de escritores | Manter estado/bloqueio de recurso; explicar efeitos parciais |
| `network_not_auditable` | Operação não oferece controle/logs exigidos | Bloquear antes de acessar o destino |
| `approval_context_stale` | Ação/alvo/revisão difere da aprovação | Não executar; obter decisão válida para a nova ação |
| `vertical_slice_unproven` | Leitura/proposta/continuação não foram integradas e comprovadas | Registrar impedimento do gate, sem marcar a fatia como aprovada |

Diagnósticos enviados à interface não devem incluir argumentos secretos, conteúdo de arquivos, URLs brutas com credenciais ou texto arbitrário de erro do provedor. Informam operação, estado e motivo seguro; detalhes técnicos seguem política de dados própria.

## 7. Invariantes para os tickets seguintes

- Tipo ou registro com campo `approved: true` vindo de uma requisição não comprova autorização.
- Responder “sim” a uma confirmação não reclassifica mecanismo reprovado como seguro.
- Remover/mover contexto invalida decisões vinculadas; capturar uma conversa não transporta seus direitos.
- Falha entre aprovação e efeito produz resultado parcial/incerto verificável, não repetição automática.
- Modos de conversa não alteram as garantias mínimas do sistema operacional, a confirmação destrutiva nem a política de rede.
- As implementações devem ser testáveis por essas interfaces quando forem construídas; adicionar uma seam nova exige o acordo previsto no fluxo de TDD.

Essas regras orientam a implementação futura. A Onda 2 não afirma que já estejam materializadas nos módulos de produção.
