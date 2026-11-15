# Modelo de ameaça

**Versão da especificação:** 0.1 (rascunho)

O acesso remoto abre, por definição, um caminho de fora para dentro de um terminal: **quem controla uma sessão remota executa comandos no desktop com o usuário dele.** Este documento diz o que o protocolo protege, de quem, e o que fica de fora.

## Ativos

| Ativo | Por que importa |
|---|---|
| **Capacidade de digitar num terminal do host** | É execução de código arbitrário no desktop |
| Conteúdo dos terminais (grade e scrollback) | Pode ter segredo: token, senha ecoada, dado de cliente |
| Estrutura do workspace (títulos, diretórios, hosts SSH) | Revela projetos, clientes, infraestrutura |
| Chave estática e de roteamento do host | Quem as tem se passa pelo host |
| Chave estática do aparelho | Quem a tem é o aparelho, até ser revogada |
| Token do operador do relay | Quem o tem registra hosts no relay |

## Adversários e o que cada um consegue

| Adversário | Capacidade | O que consegue | O que **não** consegue |
|---|---|---|---|
| **Rede** (Wi-Fi público, ISP) | Ler, atrasar, cortar tráfego | Ver que há conexão ao relay; cortar | Ler nada (TLS + Noise) |
| **Operador do relay**, ou quem o invadir | Tudo que o relay vê ([01](01-transporte-e-relay.md) §7); descartar, reordenar, injetar bytes | Metadados: IPs, horários, `host_id`, tamanho e ritmo dos quadros. Negar serviço | Ler ou alterar o conteúdo; se passar pelo host (o cliente confere a chave do QR); se passar por um aparelho (o host confere a chave na lista) |
| **Outro usuário do mesmo relay** | Um token do operador, seus próprios hosts | Registrar o próprio host | Tomar o `host_id` de outro (exige a chave de roteamento) |
| **Quem vê o QR** (ombro, foto da tela) | Os dados do QR por até 5 min; o `host_id` para sempre | Iniciar um pareamento; consultar a presença do host ([01](01-transporte-e-relay.md) §2) e saber quando o desktop está ligado | Concluí-lo sem que o usuário do desktop **confirme** um aparelho com nome e impressão digital que não são os dele. O token é de uso único |
| **Quem pega o celular desbloqueado** | O app com as chaves | Acesso total ao host até a revogação | — |
| **Quem pega o celular bloqueado** | O aparelho | Nada, se as chaves estão no armazenamento seguro com exigência de desbloqueio | — |
| **Malware no desktop** | O usuário do desktop | Tudo — já tinha | — |

## Decisões que vêm daqui

1. **Ponta a ponta sempre.** Não existe modo "só TLS até o relay". O relay é tratado como hostil para conteúdo.
2. **Pareamento exige presença no desktop.** O QR só aparece por gesto local, o token vive 5 minutos e é de uso único, e o pareamento só se conclui com confirmação no desktop.
3. **Acesso remoto desligado por padrão**, e desligar derruba tudo na hora ([02](02-handshake-e-pareamento.md) §6).
4. **Indicador visível no desktop** enquanto houver cliente conectado, com o caminho para desconectar e revogar. Quem está na frente do desktop **DEVE** conseguir saber que alguém está olhando.
5. **Revogação imediata** derruba as sessões vivas do aparelho.
6. **Erros sem eco**: o host completa o handshake `IK` antes de recusar aparelho desconhecido, e não diz ao relay o motivo.
7. **Logs sem conteúdo.** Nenhuma das três partes registra em log `payload`, teclas, grade ou títulos. O relay registra no máximo eventos de conexão.

## Riscos aceitos

| Risco | Por que aceito | Mitigação parcial |
|---|---|---|
| **Metadados de tráfego**: tamanho e ritmo dos quadros revelam quando há digitação e quanto a tela muda | Esconder exigiria preenchimento e tráfego falso constantes, contra o princípio de ocioso custar zero | O relay é auto-hospedado: o operador costuma ser o próprio usuário |
| **Celular desbloqueado nas mãos erradas** tem acesso total | O pedido do produto é acesso total, sem permissão por aparelho no v0 | Bloqueio por biometria no app (decisão do Remote), revogação imediata no desktop |
| **Sem sigilo futuro no payload da `msg1` do `IK`** | O payload vai vazio | — |
| **Negação de serviço pelo relay** | Inerente a qualquer intermediário | Auto-hospedagem |
| **Chave do host comprometida** compromete todos os pareamentos | Inerente ao modelo sem contas | "Recriar identidade" no host invalida tudo de uma vez ([02](02-handshake-e-pareamento.md) §1) |

## Fora do escopo deste modelo

- Segurança do sistema operacional do desktop e do celular.
- O que o usuário faz no terminal remoto (é o mesmo terminal do desktop, com as mesmas permissões).
- Configuração do TLS do relay (certificado, proxy reverso): é operação, documentada no [Porecatu Relay](https://github.com/porecatu/relay).
