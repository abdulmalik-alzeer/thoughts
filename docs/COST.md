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
- **So do automatic summaries.** After a chat goes quiet, or when you start another one, its text is sent once more for a short summary that Wrap later works from. It uses a small, inexpensive model, and a very long chat is read in several parts. You can switch this off in Settings → Privacy; Wrap then summarises the chats when you press it.
- **And so do suggested next messages.** After an answer in a chat, the assistant works out something you might say next and offers it under the answer. That is one more short request over the same conversation, and the answer waits a few seconds longer before it counts as finished. You can switch this off in Settings → Assistant.
- **Reading, searching and editing in the app without chatting uses nothing.**

On a subscription the practical effect of heavy use is reaching your plan's usage limit sooner, after which the provider makes you wait or offers an upgrade. On an API key the effect is a larger bill.

## The "estimated cost" figure in the app

After a run, the assistant reports how much text was processed, and Thoughts shows what that amount would cost at the provider's pay-as-you-go rates. The month total, which includes the automatic summaries, is in Settings. A chat's own figure appears beside it (on a subscription, only if you switch on “Show estimated cost” in Settings).

- **On a subscription, this is not a bill.** Your use is covered by your plan. Nothing extra is charged because of this number. Read it as a measure of how heavy a conversation was.
- **On an API key, it is an approximation** of what the provider will charge. The provider's own usage page is the authority; the figure in the app can differ from it.
- The figure is calculated on your computer from what the assistant reports. Thoughts does not bill you and does not send this number anywhere.

## A monthly spending limit

Under Settings → Privacy you can set the most you want to spend in a month, and the point at which you are warned (80% by default). At the warning point Thoughts tells you. At the limit, anything that runs without you watching stops until next month or until you raise the limit: automatic tasks, and the automatic summaries. Chats you start yourself are never stopped. The limit uses the same estimated figure as above, so on an API key the provider's own limit remains the real ceiling.

If you are on a Claude plan and reach its usage limit, Thoughts says so in plain words, with the time it resets, instead of showing an error.

## Codex

Codex reports how many tokens a run used, but not what they cost. A Codex chat shows its tokens instead of an estimated cost, Codex runs are not part of the month's figure, and the monthly limit cannot stop them: an automatic task on Codex has only its time limit. Background summaries on Codex use the model chosen for summaries (or Codex's own) at the lowest effort Codex offers. What you pay is set by your ChatGPT plan or your OpenAI account; their own usage pages are the authority.

## Keeping usage down

- **Wrap more often.** Wrap saves what matters from a conversation into your library. Then start a fresh chat: the assistant picks up from the short saved notes instead of carrying a long conversation along.
- **One topic per chat.** A new subject is cheaper in a new chat.
- **Browse in the app, not through the assistant.** Opening a note, searching, ticking todos and looking at the calendar use nothing.
- **Use Plan mode when you only want answers.** In Plan mode the assistant looks things up and replies but does not make changes, which keeps runs short.
- **Point at the thing.** "In the Acme engagement, what did we decide about fees?" lets the assistant open one item instead of searching the library.
- **Be deliberate about automatic tasks.** A daily task runs every day whether or not you read the result. Weekly is often enough.
- **Mark large folders you never discuss as private**, so the assistant does not spend time in them. See [Privacy](PRIVACY.md#private-folders).
