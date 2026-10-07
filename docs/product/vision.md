# Vision

> Mote understands what you're writing, understands enough of what you're doing, and offers the right help without making you leave your workflow.

## The problem

AI writing help lives in its own window. To fix a paragraph, sharpen a prompt or answer a message, people copy text into a chat app, wait, copy the answer back, and fix the formatting. They lose their place every time. Built-in autocorrect is local and fast but shallow, and it "corrects" Hinglish and romanized Marathi into nonsense. Prompts typed into AI assistants are often vague, and nobody stops to rewrite them.

Meanwhile, the context that would make help useful is right there: the app you're in, whether you're writing a chat reply or a prompt, the error you just copied from the browser.

## What Mote is

A quiet layer on top of every app you type in:

- It **completes** the sentence you're writing, in your tone and your language. Tab to take it, or keep typing.
- It **fixes** typos locally and checks grammar only when a sentence looks wrong, keeping your wording.
- It **improves prompts** where you write them, in ChatGPT, Claude, Gemini or your IDE.
- It **connects the dots**: copy an error, switch to your editor's AI chat, and Mote offers to turn it into a debugging prompt.
- It **shows its cost**: every request, token and estimated cent, on your machine.

## Principles

1. **Stay in the flow.** No new window to switch to. Suggestions appear at the caret and disappear when ignored. Tab and Esc are the whole interface for most interactions.
2. **Never get in the way of typing.** Debounce, cancel and cache everything. A late suggestion is dropped, never inserted.
3. **Private by default.** See as little as possible, keep only metadata, send only what a feature needs to the provider you chose, and exclude password managers and secure fields without being asked.
4. **Respect the writer.** Keep their voice, their language mix and their script. Never translate unless asked, never "fix" a word that is correct in Hindi or Marathi.
5. **Be honest about cost.** Count every request exactly once, label estimates as estimates, and show where the numbers come from.
6. **Small and calm.** No notifications, no badges, no account. A menu bar icon that stays out of the way.

## Who it's for

People who write all day across many apps:

- **Developers** who move between the browser, Slack, their IDE and AI assistants, and want better prompts and fewer context switches.
- **Professionals** writing email and chat in English, often mixed with Hindi or Marathi, who want help that doesn't flatten how they actually write.
- **Anyone** who has pasted text into a chatbot just to fix a paragraph.

## What Mote is not

- Not an autonomous agent: it never acts without a keypress.
- Not a chat app: there is no conversation window to manage.
- Not a cloud service: there is no Mote account or server, and no telemetry.
- Not a keylogger: it reads the focused field around the caret when allowed, nothing else, and never stores text.
