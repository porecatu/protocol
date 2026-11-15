# ADR-0001 — Três partes e relay cego: o host é a fonte da verdade, o relay só roteia bytes cifrados

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0002](0002-noise-para-a-sessao-ponta-a-ponta.md), [ADR-0004](0004-websocket-sobre-tls.md), [00 — Visão geral](../especificacao/00-visao-geral.md), [01 — Transporte e relay](../especificacao/01-transporte-e-relay.md), [modelo de ameaça](../especificacao/modelo-de-ameaca.md)

## Contexto

O dono do produto quer acessar uma sessão viva do Porecatu a partir do celular (Android e iOS): ver abas rodando, digitar nelas, criar e fechar abas, reorganizar grupos. O desktop costuma estar atrás de NAT, em rede de casa ou de empresa, sem IP público e sem porta aberta; o celular está em qualquer rede.

Três restrições moldam a resposta:

- **Alcance.** Desktop e celular precisam se encontrar sem que o usuário abra porta no roteador. Alguém com endereço público tem de estar no meio.
- **Sigilo.** Um terminal tem segredo na tela e nas teclas. Quem controla a sessão executa comandos no desktop. O intermediário não pode ser parte confiável para conteúdo.
- **Uma verdade só.** O Porecatu já tem um modelo de workspace rico (janelas, grupos, fixadas, abas, painéis, perfis, SSH) e um motor VT. Duplicar esse estado num servidor ou no celular cria duas fontes da verdade que divergem.

O dono decidiu, antes desta escrita: relay **cego**, **só auto-hospedado**, **sem contas**, e a tela chega ao celular como **grade já resolvida pelo desktop**, não como bytes de PTY.

## Decisão

**O sistema tem três partes com papéis fixos: o host (Porecatu desktop) é a única fonte da verdade, o relay auto-hospedado só encaminha bytes que não consegue ler, e o cliente (app mobile) só desenha e pede. O protocolo entre elas é especificado neste repositório, separado dos três.**

1. **Host.** O processo do Porecatu desktop. Conecta-se ao relay por iniciativa própria (nunca escuta porta), mantém o workspace e os PTYs, resolve a grade no próprio motor e executa as ações pedidas pelo mesmo caminho do gesto local.
2. **Relay.** Encaminha quadros entre o host e cada cliente, identificando canais. Não tem chave nenhuma da sessão, não interpreta mensagens de aplicação, não guarda nada em disco. O token do operador protege o servidor de uso por terceiros, não o conteúdo.
3. **Cliente.** Guarda só as próprias chaves e a lista de hosts pareados. Recebe a árvore do workspace e a grade dos painéis que assina; manda input semântico e pedidos de ação.
4. **Sem contas.** Identidade é chave de aparelho; o vínculo nasce de um pareamento presencial (QR na tela do desktop, confirmação no desktop).
5. **Protocolo externo.** A especificação vive em `porecatu/protocol`, com versão própria, porque é o contrato de três projetos em duas linguagens; nenhum dos três é dono dele.

## Alternativas consideradas

- **Relay confiável, só TLS até ele.** Mais simples: o relay decifra, poderia guardar histórico e servir um cliente web. Rejeitada porque o operador do relay passaria a ver teclas e telas — inclusive senhas digitadas — e o modelo de ameaça teria um ponto que, invadido, entrega todos os desktops conectados.
- **Conexão direta, sem relay (só LAN).** Zero servidor. Rejeitada para o v0 porque não atende o caso principal — acessar de fora de casa ou do escritório — e exige descoberta na rede local. Fica como evolução possível ("fora do v0"), aproveitando a mesma sessão Noise.
- **Relay público operado pelo projeto.** Melhor experiência inicial. Rejeitada pelo dono: custo, abuso, limites por usuário e obrigação de privacidade sem retorno para um projeto pessoal e livre.
- **Sessão vivendo no servidor (estilo `tmux` remoto).** O servidor rodaria os PTYs e o desktop seria mais um cliente. Rejeitada: é a "multiplexação remota" que o Porecatu declara não-objetivo, e jogaria fora o modelo de workspace e os PTYs locais que são o produto.
- **Bytes de PTY repassados, emulador no celular.** Seleção e rolagem viriam prontas de um emulador web. Rejeitada pelo dono: dois motores VT que podem divergir, e o estado inicial (tela já desenhada, modos) teria de ser reconstruído no celular.
- **Túnel genérico (SSH reverso, VPN, Tailscale).** Funciona sem protocolo novo. Rejeitada como produto: exige infraestrutura de rede do usuário, não dá a árvore de abas nem ações, e o celular precisaria de um cliente de terminal que não conhece o Porecatu.

## Consequências

### Positivas

- O conteúdo nunca existe em claro fora do desktop e do celular; invadir o relay entrega só metadados.
- Uma verdade só: o cliente não decide estado, então não há conflito de edição para resolver.
- O relay é pequeno, sem banco de dados, fácil de auto-hospedar e de auditar.
- Os três projetos evoluem com versões próprias, presos só à versão do protocolo.

### Negativas

- Com o desktop fechado não há nada a acessar: o celular mostra o host offline.
- Recursos que dependem do servidor ler conteúdo (notificação push com texto, histórico no servidor, cliente web servido pelo relay) ficam impossíveis ou exigem desenho próprio.
- O usuário precisa de um servidor com endereço público e TLS.
- Toda mudança no contrato toca até quatro repositórios.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Metadados do relay (horários, tamanhos) revelarem uso | Alta | Baixo | Auto-hospedagem; aceito no [modelo de ameaça](../especificacao/modelo-de-ameaca.md) |
| Divergência entre especificação e implementações | Média | Alto | Implementações de referência e vetores de teste ([ADR-0005](0005-implementacoes-de-referencia.md)) |
| Usuário sem servidor desistir do recurso | Média | Médio | Relay em binário único e imagem Docker, documentação de operação no repositório do Relay |
