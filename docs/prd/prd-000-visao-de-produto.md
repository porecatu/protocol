# PRD-000 — Visão de produto do protocolo

**Status:** Aprovado
**Data:** 2026-11-15
**Relacionados:** [ADR-0001](../adr/0001-tres-partes-e-relay-cego.md) a [ADR-0007](../adr/0007-toolchains.md); PRD-025 do Porecatu ([acesso remoto](https://github.com/porecatu/terminal/blob/main/docs/prd/prd-025-acesso-remoto.md))

> Decidido pelo dono do produto em 2026-11-15, antes da escrita: relay **cego** e **só auto-hospedado**, **sem contas**, a tela chega ao celular como **grade do desktop**, o **desktop dita o tamanho** do terminal, celular pareado acessa **tudo** (com indicador e revogação no desktop), **Noise** (`XX`/`IK`), **CBOR** com **CDDL**, **QR** na tela de configurações, **vários desktops** por aparelho, **quatro repositórios** com a especificação neste, e **GPL-3.0-or-later** em todos.

## Problema

O Porecatu organiza muitos terminais: grupos, abas, painéis, sessões. Quando o usuário sai de perto do desktop, essa organização fica inacessível: não dá para ver se o build terminou, responder a um prompt, ou abrir uma aba para rodar um comando rápido.

As saídas que existem hoje custam caro: abrir porta no roteador e rodar um servidor SSH (e perder a árvore de abas, vendo um shell solto), usar uma ferramenta de acesso remoto de tela inteira (pesada no celular e alheia ao terminal), ou montar `tmux` dentro do Porecatu só para poder reanexar — a camada extra que o Porecatu existe para evitar.

## Proposta

Um protocolo pequeno e fechado que deixa um app mobile **anexar à sessão viva do Porecatu**, atravessando um relay auto-hospedado que **não consegue ler nada**.

Ele é o contrato de três projetos:

- **Porecatu** (host): passa a se conectar a um relay e servir a árvore do workspace e a grade dos painéis;
- **Porecatu Relay**: encaminha bytes cifrados;
- **Porecatu Remote**: o app que desenha e pede.

## Usuários

- **Quem usa o Porecatu** e quer acompanhar e agir nas abas pelo celular, fora de casa ou do escritório.
- **Quem implementa** um dos três projetos: precisa de um texto normativo, de vetores de teste e de uma implementação de referência na sua linguagem.

## Princípios

1. **O host é a verdade.** O cliente não interpreta VT nem decide estado.
2. **O relay é cego.** Conteúdo nunca em claro fora das pontas; metadados reconhecidos e documentados.
3. **Identidade é chave.** Sem contas, sem senha; pareamento presencial.
4. **Mesmo gesto, mesmas regras.** Ação remota é executada pelo caminho do gesto local, com as mesmas confirmações.
5. **Ocioso custa zero.** Sem assinatura, sem leitura do motor; sem mudança, sem mensagem.
6. **Nenhuma prosa no fio.** Códigos tipados; o texto é de quem mostra.
7. **Pequeno e verificável.** Cada mensagem tem vetor; duas implementações concordam byte a byte.

## Escopo do v0

| # | Capacidade | Onde |
|---|---|---|
| 1 | Host se registra num relay auto-hospedado com token do operador | [01](../especificacao/01-transporte-e-relay.md) |
| 2 | Pareamento por QR, com confirmação no desktop | [02](../especificacao/02-handshake-e-pareamento.md) §3 |
| 3 | Conexão de aparelho pareado em um round-trip | [02](../especificacao/02-handshake-e-pareamento.md) §4 |
| 4 | Árvore do workspace (janelas, fixadas, grupos, abas, painéis), perfis e tema | [03](../especificacao/03-mensagens.md) §4 |
| 5 | Grade ao vivo por assinatura, com controle de fluxo, e scrollback por página | [03](../especificacao/03-mensagens.md) §5 |
| 6 | Input semântico codificado pelo host | [03](../especificacao/03-mensagens.md) §6 |
| 7 | Criar e fechar abas e painéis, renomear, organizar grupos, mover entre janelas, iniciar painel | [03](../especificacao/03-mensagens.md) §7 |
| 8 | Revogação imediata de aparelho | [02](../especificacao/02-handshake-e-pareamento.md) §6 |

## Não-objetivos do v0

Cada um é decisão, não esquecimento.

- **Notificações push.** Exigem APNs/FCM com credenciais no relay auto-hospedado e um payload que continue cego. Fora do v0.
- **Conexão direta na rede local.** Mesma sessão Noise serviria; descoberta e transporte novos não cabem agora.
- **Cliente web.** Servir um app pelo relay daria ao operador do relay o poder de trocar o código que faz a criptografia — contradiz o relay cego.
- **Pareamento por código digitado.** Um código curto exige PAKE (ex.: SPAKE2) para não ser quebrado por força bruta; o QR resolve o caso comum.
- **Permissões por aparelho** (somente leitura, só algumas abas). O pedido é acesso total, com indicador e revogação.
- **Modo em segundo plano do host.** Sem Porecatu aberto, o host está offline; manter o processo vivo sem janela é decisão do ciclo de vida do Porecatu.
- **Abrir janela, mudar o painel focado ou dar foco a uma janela do sistema do desktop, fixar, sessões nomeadas, configurações, gerenciador de tarefas** pelo celular.
- **Responder perguntas do backend SSH** (senha, chave do servidor) pelo celular.
- **Mouse** no terminal a partir do celular.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Abrir o app até ver a grade de um painel já pareado, em 4G | < 1 s (um round-trip de `IK` + `hello` + snapshot) |
| Tecla no celular até eco na tela do celular, relay no mesmo continente | < 150 ms |
| Tráfego com painel ocioso | zero mensagens de grade |
| Divergência entre implementações | zero vetores falhando nas duas, sempre |

## Riscos

| Risco | Mitigação |
|---|---|
| Implementação TS do Noise com bug de segurança | Vetores do `cacophony` e cruzados; revisão dedicada ([ADR-0002](../adr/0002-noise-para-a-sessao-ponta-a-ponta.md)) |
| Usuário sem servidor próprio | Relay em binário único e Docker, documentação de operação |
| Protocolo crescer por pedidos de recurso | Escopo fechado do v0; mensagem nova só com ADR e vetor |
| Conflito GPL × App Store | Risco aceito ([ADR-0006](../adr/0006-licenca-gpl.md)) |
