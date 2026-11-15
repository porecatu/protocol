# ADR-0002 — Noise para a sessão ponta a ponta: `XX` no pareamento, `IK` nas conexões seguintes

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0001](0001-tres-partes-e-relay-cego.md), [ADR-0005](0005-implementacoes-de-referencia.md), [02 — Handshake e pareamento](../especificacao/02-handshake-e-pareamento.md), [modelo de ameaça](../especificacao/modelo-de-ameaca.md)

## Contexto

O relay é cego ([ADR-0001](0001-tres-partes-e-relay-cego.md)): host e cliente precisam de um canal cifrado e mutuamente autenticado **por cima** do relay, que o relay não consiga ler, alterar nem personificar. Os requisitos concretos:

- **Autenticação mútua por chave de aparelho**, sem contas nem senha: o host reconhece aparelhos pareados; o aparelho reconhece o host pela chave que leu no QR.
- **Sigilo futuro**: a chave de um lado vazada depois não pode decifrar sessões gravadas antes.
- **Um round-trip** nas conexões do dia a dia: o celular abre o app e quer a tela já.
- **Duas implementações**, Rust (desktop) e TypeScript no Expo (celular), sem módulo nativo no app se possível.
- **Nada de criptografia inventada.** O projeto não tem criptógrafo; o handshake tem de ser um padrão analisado, não uma composição própria de primitivas.

## Decisão

**A sessão ponta a ponta é Noise, com `Noise_XX_25519_ChaChaPoly_BLAKE2s` no pareamento e `Noise_IK_25519_ChaChaPoly_BLAKE2s` nas conexões de aparelho já pareado, ambos iniciados pelo cliente. Rust usa `snow`; TypeScript implementa o Noise sobre `@noble/curves`, `@noble/ciphers` e `@noble/hashes`.**

1. **Pareamento em `XX`.** O cliente ainda não é conhecido pelo host; o `XX` troca as duas estáticas cifradas. O cliente compara a estática revelada pelo host com a do QR antes de mandar a terceira mensagem — é isso que amarra o canal ao desktop certo mesmo com relay hostil. O token de uso único e o nome do aparelho vão cifrados na terceira mensagem.
2. **Conexão em `IK`.** O cliente já conhece a estática do host (do QR): um round-trip, e a estática do aparelho viaja cifrada na primeira mensagem. O host sempre completa o handshake e só então recusa aparelho desconhecido, com resposta autenticada.
3. **Prólogo** `"porecatu-remote/<major> " || host_id`: amarra a sessão ao endereço e à versão maior.
4. **Primitivas**: X25519, ChaCha20-Poly1305 e BLAKE2s — todas disponíveis em JS puro nas bibliotecas `@noble` (auditadas, sem dependências) e no `snow`.
5. **Registros** com 1 byte de flags (fragmentação e rekey) e mensagens de até 16 MiB, conforme [02](../especificacao/02-handshake-e-pareamento.md) §5. Rekey pelo `Rekey()` do Noise, sinalizado em banda.
6. **Chave de roteamento separada** (Ed25519) só para o registro no relay: a estática X25519 não assina, e misturar papéis numa chave só complicaria a troca de uma delas.

## Alternativas consideradas

- **Construção própria sobre libsodium** (`crypto_kx` para troca de chaves + `crypto_secretstream` para o fluxo). Bibliotecas maduras dos dois lados (`libsodium` em Rust, `react-native-libsodium` no app). Rejeitada porque o handshake — autenticação mútua, amarração ao QR, sigilo futuro, recusa sem oráculo — ficaria por nossa conta, e é exatamente a parte que o Noise já especifica e foi analisada formalmente. Além disso, `react-native-libsodium` é módulo nativo, o que exige build próprio do Expo.
- **TLS 1.3 ponta a ponta tunelado** (certificado autoassinado do desktop fixado no pareamento, cliente com certificado próprio). Familiar e padrão. Rejeitada: embutir uma pilha TLS completa no Expo, operando sobre um fluxo que não é socket, não tem biblioteca madura em JS; autenticação mútua por certificado de cliente é pesada de administrar; e o TLS traz negociação de suítes e extensões que o protocolo não precisa.
- **`IK` também no pareamento.** O cliente já conhece a estática do host pelo QR, então daria. Rejeitada porque a primeira mensagem do `IK` cifra com chaves sem sigilo futuro, e o pareamento é o momento em que o token e o nome do aparelho viajam; no `XX` eles vão na terceira mensagem, já com sigilo futuro. Custa um round-trip a mais, uma vez por aparelho.
- **`XX` em toda conexão.** Um protocolo só. Rejeitada pelo round-trip extra em toda abertura do app.
- **Signal/MLS ou similares.** Pensados para mensageria assíncrona e grupos; complexidade sem ganho num canal síncrono entre dois pontos.

## Consequências

### Positivas

- Handshake especificado e analisado; o projeto implementa um padrão, não inventa um.
- Autenticação mútua por chave, sem contas, com sigilo futuro e um round-trip no uso diário.
- O app não precisa de módulo nativo para criptografia: roda no fluxo gerenciado do Expo.
- Os vetores de teste do próprio Noise (`cacophony`) validam as duas implementações antes dos vetores do Porecatu.

### Negativas

- Não há biblioteca Noise madura em TypeScript: a implementação TS do estado de handshake (algumas centenas de linhas sobre `@noble`) é nossa, e é código de segurança.
- Perder o celular sem bloqueio entrega acesso até a revogação; não há segundo fator no v0.
- Trocar a estática do host desfaz todos os pareamentos.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Bug na implementação TS do Noise | Média | Alto | Vetores do `cacophony` e vetores cruzados com o `snow`; revisão dedicada; código isolado num módulo pequeno do pacote |
| Ataque de tempo na comparação da chave do QR ou do token | Baixa | Médio | Comparações em tempo constante exigidas pela especificação |
| `snow` ou `@noble` com vulnerabilidade | Baixa | Alto | Versões exatas, auditoria (`cargo audit`, `pnpm audit`) no CI, subida como tarefa própria |
| Celular perdido | Média | Alto | Armazenamento seguro com exigência de desbloqueio, revogação imediata no desktop |
