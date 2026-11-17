---
name: email-writing
description: "Draft an email to someone the user writes to, in that person's language and the tone they prefer, ending with an Open in Mail link; nothing is ever sent. Use when the user says \"write an email to …\", \"email …\", \"reply to …\", \"draft a message to …\" or pastes an email to answer."
---

# Email writing

You write the draft; the user reads it and sends it from their own mail app. Speak plainly and keep questions to a minimum.

## Steps

1. **Find the person.** Look in `people/` for the recipient: open `people/README.md` and match on the name or the email address the user gave (allow for nicknames, first names only and different spellings), then read that person's file, `people/<name>.md`. Several matches: ask which one, in one short question. If the `people/` folder cannot be opened (for example because it is a private folder), do not try another way in: ask the user for what you need instead.
2. **Someone new.** If the person is not in the list, ask once, in one batch: their email address, who they are (role and organisation), how the user knows them, and the language and tone to write in. Then offer to add them to People so next time is quicker. Only if the user agrees, write `people/<name>.md` (the name in lowercase Latin letters and dashes, for example `sara-al-qahtani.md`) in exactly this shape:

   ```markdown
   ---
   name: {Full name}
   email: {address, or leave empty}
   role: {Role}
   organisation: {Organisation}
   relationship: {client, colleague, supplier, friend…}
   language: {Language to write to them in}
   tone: {formal, friendly…}
   updated: {today, YYYY-MM-DD}
   summary: {One line: who they are.}
   ---

   # {Full name}

   {Anything the user said that helps when writing to them.}
   ```

   and add one line to `people/README.md` (create it with a `# People` heading if it does not exist): `- [{Full name}]({file name}) · {role}, {organisation} · {email}`. Change nothing else in an existing person's file without asking.
3. **Understand the email.** What it is for, the key points, any dates, figures or attachments to mention. If the user pasted an email to reply to, answer what it actually asks. Ask only about what is missing and matters; otherwise write.
4. **Draft it** in the language the person's file gives (otherwise the language the user wrote in) and in the tone it gives (otherwise polite and plain). Use the greeting and sign-off that fit that language and tone. Keep it short; keep names, figures and dates exactly as given; promise nothing the user did not say.
5. **Show it** as a subject line and the body, ready to copy.
6. **Open in Mail.** End with one link written as `[Open in Mail](mailto:…)`: the address, then `?subject=` and `&body=`, with the subject and body percent-encoded (a space is `%20`, a line break `%0D%0A`, and `&`, `?`, `#`, `%`, `+` and every non-Latin letter encoded as UTF-8). If the whole link would be longer than about 1,800 characters, give the link with the subject only and say: "Paste the body into the message after it opens." No address known: leave it out of the link and say the user can fill it in.
7. **Never send anything.** You do not send email or messages, sign in to any mail service, or ask for a password. The user presses Send in their own mail app.
8. **Keep a copy, if asked.** Offer, in one line, to save the draft with the matching item (its `outputs/` folder) or under `people/`. Only on a yes, save it as a dated note, for example `outputs/YYYY-MM-DD-email-to-sara.md`.
