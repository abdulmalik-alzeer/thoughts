# Working with notes, chats and files

The everyday tools around your notes and chats: looking at pictures and files, printing, reading a note beside a chat, changing a message you sent, a second window, the Graph, and the moments when Thoughts needs you.

العربية: [docs/ar/USING.md](ar/USING.md)

## Pictures

Click any picture to see it large: one attached to a chat or shown in an answer, a picture in a note, a picture file in **Docs**, a picture inside a Word document, or one on an item's page or in Help.

- **Zoom** with the scroll wheel, by pinching on a trackpad, or with **+** and **−** (the buttons at the bottom, or the keys). The middle button shows the zoom level; click it to see the whole picture again.
- **Move** a zoomed picture by dragging it, or with the arrow keys.
- **Double-click** switches between the whole picture and its real size.
- **Close** with **Esc** or the **X** button. Clicking beside the picture does not close it, so a drag that ends off the picture never loses your place.

## Previewing files

**Docs**, and the files attached to a chat, show most kinds of file right inside the app:

- pictures, PDFs and SVG drawings (shown as a picture);
- Word documents, OpenDocument text and Rich Text;
- Excel and OpenDocument spreadsheets, CSV and TSV, as tables with a tab for each sheet (the first 1,000 rows and 60 columns);
- PowerPoint and OpenDocument presentations, slide by slide with their notes;
- text and code, with colours, JSON (as a tree or as text), YAML and XML;
- web pages, as their text or with **Show as page**: the page is then shown without its scripts, forms or anything from the internet;
- sound and video;
- the list of files inside a ZIP archive.

Nothing in a file runs while it is previewed: Office files with macros are only read.

Some files are not shown, and the preview says why: files over 25 MB (less for text), files protected with a password, and files in the **older Word, Excel and PowerPoint formats** (doc, xls, ppt). To preview one of those, open it in its program and save it again in the current format.

Old Office files, Rich Text and OpenDocument files can carry macros, so Thoughts does not open them in another program with one click. It offers **Show in Finder** instead (**Show in File Explorer** on Windows, **Show in folder** on Linux), and you open the file from there yourself. Opening a file from a private folder in another program asks you first.

## Diagrams and charts

Ask for a picture and the answer has one in it: "draw how the checkout talks to the bank", "show last quarter's sales as bars". The same pictures are drawn in your notes, wherever a note holds one.

- **Diagrams** are drawn by [Archify](https://github.com/tt-a1i/archify), which comes inside Thoughts: architecture, workflows and flowcharts, sequences of messages, data flows, lifecycles (states), database tables, classes, trees, timelines and waterfalls.
- **Charts** of numbers are drawn by Thoughts itself: bars (also stacked or sideways), lines, areas, pies, donuts and scatter plots.

Above each picture:

- **SVG** saves it as an SVG picture, sharp at any size, for a document, a slide or a web page.
- **draw.io** (diagrams only) saves it as a file you can open and change in draw.io (diagrams.net): every box is a shape you can move, and the lines stay attached to the boxes they join.
- **Source** shows the text the picture was drawn from. To change a picture, ask the assistant for the change, or edit that text in the note.

Click a picture to see it large. Pictures follow the app's day or night look, and are printed with the note.

**Diagrams need Node.js** (version 18 or newer) on your computer. Without it a diagram shows its source and says so; charts are drawn either way. Get it from [nodejs.org](https://nodejs.org). Nothing is sent anywhere to draw a picture: it all happens on your computer.

When a diagram cannot be drawn, Thoughts shows what is wrong with it. **Copy message**, paste it to the assistant, and it corrects the diagram.

## Slides

Any note can be a slide deck. Put `slides: true` at the top (in the frontmatter, between the two `---` lines) and separate the slides with a line holding only `---`, with an empty line before and after it. A line starting with `Note:` begins that slide's speaker notes. Diagrams, charts and pictures work as they do in any note. Ask the assistant for a deck ("make slides about our quarterly review") and it writes one this way; in a chat it shows as a card with **Present** and **Save as note**.

**Present** (in Docs, in a file preview, on the card, or from the command palette) shows the deck full screen. The arrow keys, Space and Page Up/Down move between slides, **O** shows every slide, **S** opens the presenter panel (your notes, the next slide, a timer) and **Esc** leaves. A deck written in Arabic runs right to left by itself. With "Reduce motion" on (Settings › Accessibility) slides change without animation.

**More › Save slides as HTML** in Docs saves the deck as one file in the idea's `outputs` folder. It opens in any web browser, on any computer, with pictures and charts inside it and nothing loaded from the internet.

## Plan and build a project (MADO)

Thoughts can take a software idea from a first conversation to working code, one step at a time, with you reading and approving each step. Open **Projects**, pick a project folder you linked, and choose **Build with MADO**. Planning starts a `mado` folder inside the project; everything the plan says lives there.

Once a plan is started it is listed in the sidebar under **Plans**, below your chats, with the step it is at; a small ring shows while something is being written or built, and a filled dot when a build waits for you. Click one to open it.

At the top of the plan, a button names **the model and the effort** every step of this plan and its builders use (Default model until you choose; the lists are the ones your chats offer). Beside it is the same usage ring the chats have.

The seven steps, in order:

1. **Brief**: a short interview about what you want to make, written up as a one-page summary.
2. **What it does**: the features, one by one, and a diagram of the screens and how people move between them.
3. **How it's built**: the parts of the system and how they talk to each other, and the data it keeps (each as a diagram). A **Conventions** tab holds the one way this project builds a screen, handles an error and names things; every builder is given it.
4. **Look and screens**: colours, type and spacing, and a first picture of every screen.
5. **Work plan**: the work cut into tasks, each saying which files it changes and which command checks it is done. Two kinds of task come before the features: **Agreed examples** (an example of every answer the parts give each other, so they cannot drift apart) and **Tests first** (a feature's tests, written before the feature); once done, later tasks may not change their files. **How the build checks itself** lists what is run on the whole project after every round, and how the product is started so its screens can be looked at.
6. **Final check**: the app checks the plan against what it should do (every feature and every screen has work planned for it, nothing waits on itself) and lists every command the build would run.
7. **Build**: the app does the work.

A step opens once the steps before it are approved (**How it's built** and **Look and screens** both open after **What it does**). Each step has its own chat, so you can talk to the assistant about it; approving a step again after a change marks the later steps for another look.

**Comments are how you ask for changes.** Select a passage and choose **Comment**, or comment on a box or a line of a diagram, a design setting or a spot on a screen. Collect as many as you like, then **Send changes**: the assistant gets them all as one request, changes the documents and the diagrams together, and the comments are marked done.

**Diagrams can be edited.** Rename a box, add one (it is placed next to what you selected), draw a line between two boxes, delete something, or fill in a table's fields. Your edits show as pending until you **Send changes**; the assistant then applies them to the diagram and to the documents that describe the same thing. Moving boxes is saved at once and changes nothing else. The tasks diagram is drawn from the work plan and cannot be edited.

**The design step** shows a few ready-made looks and the assistant's own; pick one, then change any colour, font, corner or spacing yourself. **Fix contrast** makes text easy to read again. The screens are pictures only: they are shown in a sealed frame that runs nothing and loads nothing from outside.

**Approving the final check allows the app to run the listed commands on this computer.** Read them: the build runs exactly these commands, set-up commands first (installing what the project needs, for example), then each task's check. These commands run code the assistant writes during the build: a test command runs the project's tests, whatever they contain. They run in a separate copy of the project, never in your own folder, and they can use the network. Thoughts asks you itself, in a dialog of its own that names the project and lists the commands: the app window alone cannot approve it. If the plan changes after you looked, the approval is refused and you are shown the new list.

**What those commands are kept from.** On a Mac, while **Settings › Privacy & cost › Keep the build's commands to its copy of the project** is on (it is unless you switch it off), they can change files only in the build's copy of the project, in temporary files and in the build's own download cache (not your usual npm, Cargo or other caches); they cannot read your own files, your saved sign-ins or keys, or the app's data, and cannot reach the app itself. One consequence: a project that installs packages from a private registry with your saved credentials fails to install while it is on. You can switch it off for such a project (Thoughts asks first, because the protection is then much weaker: the commands run as you) and back on afterwards. On Windows and Linux, and while full access is on, the commands are not confined; the build view says so.

**The build** needs the project to be a git repository; the Final check tells you when it is not, and cannot be approved until it is. As its first step the build commits whatever you have not committed yet, on your own branch, as “Before the MADO build” (the Final check lists those files beforehand; the `mado` folder is left out). After that it does not touch your own branch while it works: it builds on a branch of its own (`mado/build-1`, …), a few tasks side by side, and brings the pieces together. The assistant only reads and edits files for each task; it runs no commands itself. The app runs each task's check when the assistant is done; when one fails, it goes back to the assistant in the same chat with what the check printed, up to three times, and then the build waits for you (**Try again**, or **Resume**). You can **Pause** or **Stop** it at any time. A build task's chat is a record you can read, not continue. When the plan names checks of the whole project, the app runs them after every round on everything built so far (**Whole project checked**); when they fail, a builder fixes what they find, up to three times, and then the build waits for you. When every task is done, two more things happen by themselves — for a plan made with this version; a plan made by an earlier version is built exactly as before and is done after its last round. **Looking at the screens**, when the plan says how to start the product: the app starts it in the build's copy and opens each screen in a hidden window that can reach nothing else, noting what it shows and any error (only the desktop app can look; otherwise this is skipped and the build goes on). **Review**: an assistant that built none of it reads the result against your features, the conventions and your comments; what it says must be fixed is fixed, then it reads once more. What it still questions is listed as **Must fix** or **Worth a look**, and you can bring the build in anyway. When everything is done, **Bring the finished build into my branch** merges it. The Build step then shows what was brought into which branch, how many points the review left open, and **How to run it**: the start command (with **Copy**), the address to open, and the project's own README. Thoughts does not start your project for you. **Plan a change** takes you back to the steps. It also slows down by itself near your plan's usage limit (Settings › Privacy and cost sets where).

**What stops a build.** A link, a special file or a file shared with another place inside the `mado` folder (a downloaded project can carry one) stops planning and building until it is replaced by a plain file. So do git settings in the project's own `.git` folder that make git start other programs or read settings from elsewhere (filters, merge or diff programs, an editor or pager, a sign-in helper, signing, includes); the message names them.

**Limits.** Planning and build runs always use Claude Code (not Codex), unless full access is on. A planning run may write only the `mado` folder; anything it changes elsewhere is listed in the step, and the step cannot be approved until you have looked at it and dismissed the notice.

## Printing and saving as a PDF

Any note can be printed or saved as a PDF, in a light print layout whatever theme you use, with Arabic text laid out correctly. The PDF is named after the note's title.

- **In Docs:** choose **More › Save as PDF…**. To print, choose **Print this page** in the command palette.
- **On an item's page and in its log:** use the **Print…** and **Save as PDF…** buttons.

**On a Mac**, **Save as PDF…** asks where to save and writes the file straight there. A message then offers **Open**.
**On Windows and Linux**, the print window opens. Choose "Save as PDF" (or "Microsoft Print to PDF") as the printer, then save.

## A note beside the chat

While you chat, you can read a note without leaving the conversation. Click a note in the list of notes an answer changed, a link to one of your notes in an answer, or a note you mentioned with @, and it opens in a pane beside the chat.

- It follows the note: when the assistant changes it again, the pane shows the new version.
- The pane is for reading. **Open in Docs** opens the note where you can change it.
- Drag the pane's edge to make it wider or narrower (or use the arrow keys on the edge). Thoughts remembers the width.
- **Close** (or **Esc**) closes it. Switching to another chat closes it too. In a narrow window the pane covers the chat until you close it.
- To go straight to Docs instead, hold **⌘** (**Ctrl** on Windows and Linux) as you click, or click with the middle button.

## Editing a message, and branches

In an ordinary chat, point at a message to see the buttons under it: **Edit** on your own messages, and **Branch** on your messages and on the assistant's answers. (Wrap, setup and customisation chats do not have them.)

**Edit your last message.** Change the text, and choose which of the files sent with it to keep, then press **Send again**. The new message takes the place of the old one, and its answer is replaced. If the answer is still being written, it stops first. Changes that answer already made to your library stay: undo them in History first if you want them gone.

**Edit an earlier message.** The change goes into a **branch**, and the button says **Send in a branch**. The chat you were in stays exactly as it was.

**Branch from any message.** **Branch** makes a new chat with the conversation up to that message or answer, and you carry on there in another direction.

What a branch gets, and what stays in the original:

- The branch has the conversation up to that point and copies of the files attached to it. Its title ends in "(branch)". At its top, **Branched from** links back to the message it starts from, and the original chat lists its branches.
- The original chat is not changed in any way.
- Each part is saved by one Wrap: what came before the branch is saved with the original chat, and only what you add in the branch is saved from the branch. If you delete the original before wrapping it, what came before the branch is not saved from either chat.
- How the assistant continues: with Claude Code, a branch made at the latest message carries on from the assistant's own record of the conversation. A branch made at an earlier message gets the conversation so far as text: the assistant reads what was said, but not the files it looked at along the way. A very long conversation is passed on as a summary plus the last messages word for word.
- **Codex** cannot copy its own record of a conversation, so a Codex branch always gets the conversation as text.

## A second window

To keep two notes, or a chat and a note, side by side, open one in a window of its own:

- **A note:** in Docs, choose **More › Open in new window**. Save or discard your changes to the note first.
- **A chat:** press the window button at the top of the chat (**Open in new window**).

A second window is the whole app, with the sidebar folded away. Some things stay with the main window, and a second window brings it to the front for them: **Settings**, **Customise**, checking for updates, and the system confirmations for bypassing permissions or trusting a folder. A second window also does not show notifications, reminders or the count on the Dock, and has no dictation. Closing a second window never quits Thoughts. When you quit, Thoughts checks for unsaved work in every window.

Second windows exist only in the desktop app.

## The Graph

**Graph** in the sidebar shows your notes as a map: each dot is a note, each line a link from one note to another. Click a dot to open its note.

- **Find a note…** searches the map; the arrows next to it go through the matches.
- The area buttons show or hide an area (double-click one to show only that area). **Status** shows notes of one status only.
- Zoom with the buttons or the scroll wheel, drag to move around, and press **Fit** to see everything again.
- **Show as list** gives the same connections as a list, which is easier with a keyboard or a screen reader. You can also move from note to note on the map with the keyboard.
- In a large library the map shows the best-linked notes and says so. Choose an area or a status to see the rest.

On an item's page, **Links** opens the links around that page: the notes it links to and the notes that link to it. **How far to look** goes further, one link at a time.

## When the assistant needs you

When the assistant is waiting for your OK in a chat:

- the chat gets a mark in the sidebar, a filled dot that says "waiting for your OK" to screen readers;
- while Thoughts is in the background, the Dock icon shows how many questions are waiting, and bounces once for each new one (on Windows the taskbar button flashes);
- the message "The assistant is waiting for your OK" stays on screen until you answer, the question runs out or the answer ends.

The mark goes as soon as you answer. Clicking a Thoughts notification opens the chat it is about.

**Confirmations the system shows.** Three choices are confirmed in a dialog that the Thoughts app itself shows, not the page inside its window: switching a chat to bypass permissions (for Codex, giving it full access), making bypass the default for new chats, and trusting a folder. The dialog names the chat or the folder (with its full location), and nothing running inside the window can answer it. You confirm once: in the trust question, **Trust…** opens this dialog, and its **Trust** is your yes. **Cancel** changes nothing. See [Privacy](PRIVACY.md#the-trust-question-when-opening-a-library).

**Full access (dangerous).** In **Settings › Privacy & cost › Full access** you can let the assistant, and every add-on or page, do anything in the app and on this computer, with no limits added by Thoughts: every chat, Wrap, automatic task and customisation then runs without permission questions or sandbox, private folders and the app's own data are open to it, the customisation layer is no longer guarded (built-in pages, the Recovery menu and Rollback can be changed or removed), and pages get a bridge to run programs, read and write any file and open apps. Only what the assistant program itself declines stays out of reach. It is off unless you switch it on, and only you can: Thoughts asks in two dialogs of its own, and in the last one the button that is pressed by Enter keeps it off. While it is on, a red **Full access on** sign stays in the sidebar; one click on it and one on **Turn off** switch it off. Off restores every protection at once for new work and stops any assistant run or page command that was started with full access. If you keep a copy of the Thoughts source code, you can name its folder there, so the assistant can change and rebuild the app (it does not rebuild itself). The time it was switched on is written to the app's log.

## Changes from your other computers and other apps

**Changes waiting.** If your library has an online backup or a drive backup, Thoughts can tell you when your other computers have sent changes. Switch on **Check for changes from your other computers** in **Settings › Library & backup**. It is off until you switch it on. Every 15 minutes while Thoughts is open, the app looks at your main backup location and shows **N changes waiting** beside **Sync**. It only looks: nothing in your notes changes until you press **Sync**.

For a location that needs a sign-in, the check uses the sign-in you saved and never asks for your password. It starts only once a **Sync** has worked since Thoughts was started; until then Settings says **Sync once to start checking**. When the check cannot reach the location (offline, or a sign-in that no longer works), it says so quietly in Settings and tries again less often, up to every 4 hours.

**Changed outside Thoughts.** When a note changes outside the app, in another program, on your phone or through iCloud, what you see refreshes, and History records the change as its own entry, **Changed outside Thoughts**. See [History](HISTORY.md#what-history-shows).

## Suggest automations

**Customise › Automate** can suggest routines, helpers, connections and new pages from how you use Thoughts. Nothing is sent until you press **Suggest automations**, and you can read exactly what will be sent first. See [Suggest automations](CUSTOMISING.md#suggest-automations), and [Privacy](PRIVACY.md#what-is-sent-to-the-ai-provider) for what leaves your computer.

## Search by meaning

**What it is.** Word search finds notes that contain your words. **Search by meaning** also finds notes about what you mean when the words differ, in Arabic and English: an Arabic question can find an English note about the same thing. It runs on this computer; nothing about your notes is sent anywhere.

**Off until you switch it on.** In **Settings › Features › Search by meaning**, the app shows the model it uses (Multilingual E5 small, about 465 MB, MIT licence) and **Switch on and download**. Nothing is downloaded before you press it. The model comes from Hugging Face, where it is published, and is checked against its known fingerprint before it is used. The download shows a bar like an app update's; **Pause** stops it and **Continue** goes on from where it stopped (also after a restart or a dropped connection); **Cancel** deletes what was downloaded.

**Getting your notes ready.** Once the model is in, the app reads your notes in the background, outside private folders, and shows **Getting your notes ready: N of M**. You can go on working. After that it follows your changes by itself. Making a folder private takes its notes out at once.

**Using it.** The search page shows **Related by meaning** under the word results (notes already found by their words are not repeated). Ordinary chats are told which notes may relate to your message, by their names only; the assistant reads them if they help.

**When the app suggests it.** Never switched on by itself. It may suggest it when your library is large, when a search of two or more words finds nothing, or when the assistant could not find what you meant (a card under its answer: **Set it up**, **Not now** for 30 days, or **Don't ask again**).

**Removing it.** **Remove search by meaning** deletes the model and what was made from your notes, says how much space that frees, and switches it off. Your notes are not touched.

