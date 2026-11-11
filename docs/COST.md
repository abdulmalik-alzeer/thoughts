# What Thoughts costs to run

Thoughts itself is free. The assistant inside it is not: it runs through your own account with an AI provider, and that is where any cost comes from.

العربية: [docs/ar/COST.md](ar/COST.md)

## What you need

One of these:

- **A Claude plan that includes Claude Code.** You pay the provider a fixed subscription, and your use of the assistant in Thoughts counts against that plan's usage allowance. This is the simpler choice for most people.
- **An API key.** You pay the provider for what you use, measured by how much text goes back and forth. There is no fixed fee and no fixed ceiling unless you set one in your account with the provider.

Thoughts does not sell either, takes no cut, and never sees your payment details.

## Where to see current prices

Prices and plan names change, so this page does not quote them. Look at the provider's own pages:

- Claude plans: [claude.com/pricing](https://claude.com/pricing). Check that the plan you pick lists Claude Code.
- API pricing and spending limits: your account with the provider (for Claude, the Anthropic Console).

If you use a different provider through the address field in Settings, that provider's prices apply.

## How to think about cost

What drives usage is how much the assistant has to read and write, not how many messages you send.

- **Long conversations cost more per message than short ones.** Each new message is answered with the whole conversation so far in view.
- **Reading files adds up.** A question that makes the assistant open twenty notes uses more than one about the note in front of you.
- **Work done for you in the background counts too**: Wrap, automatic tasks and customisation requests are all assistant runs.
- **Reading, searching and editing in the app without chatting uses nothing.**

On a subscription the practical effect of heavy use is reaching your plan's usage limit sooner, after which the provider makes you wait or offers an upgrade. On an API key the effect is a larger bill.

## The "estimated cost" figure in the app

After a run, the assistant reports how much text was processed, and Thoughts shows what that amount would cost at the provider's pay-as-you-go rates. It appears beside a chat and as a month total in Settings.

- **On a subscription, this is not a bill.** Your use is covered by your plan. Nothing extra is charged because of this number. Read it as a measure of how heavy a conversation was.
- **On an API key, it is an approximation** of what the provider will charge. The provider's own usage page is the authority; the figure in the app can differ from it.
- The figure is calculated on your computer from what the assistant reports. Thoughts does not bill you and does not send this number anywhere.

## Keeping usage down

- **Wrap more often.** Wrap saves what matters from a conversation into your library. Then start a fresh chat: the assistant picks up from the short saved notes instead of carrying a long conversation along.
- **One topic per chat.** A new subject is cheaper in a new chat.
- **Browse in the app, not through the assistant.** Opening a note, searching, ticking todos and looking at the calendar use nothing.
- **Use Plan mode when you only want answers.** In Plan mode the assistant looks things up and replies but does not make changes, which keeps runs short.
- **Point at the thing.** "In the Acme engagement, what did we decide about fees?" lets the assistant open one item instead of searching the library.
- **Be deliberate about automatic tasks.** A daily task runs every day whether or not you read the result. Weekly is often enough.
- **Mark large folders you never discuss as private**, so the assistant does not spend time in them. See [Privacy](PRIVACY.md#private-folders).
