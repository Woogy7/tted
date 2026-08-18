# Working with an agent in TTED

Press **F9**. That is the whole entry point.

If Codex is installed and already signed in, the pane becomes ready
automatically. Otherwise TTED shows a simple setup card:

- if Codex is missing, click the card to display the official install command;
- if sign-in is needed, click **Sign in with ChatGPT**;
- open the displayed address and enter the code; clicking copies the code.

Once the title says **Codex — ready**, type what you want and press Enter.
Shift+Enter adds a line without sending. TTED automatically includes the current
file, cursor, and selection as context.

Codex can inspect and edit only the current workspace. Its answer, commands,
file-edit activity, errors, and completion state appear in the pane. Files
refresh in the normal editor area when work finishes.

Human messages are labeled **YOU** on a tinted background; replies are labeled
**CODEX**, while activity and errors use quieter dot and warning rows. Scroll
conversation history with the mouse wheel or Page Up/Down. Home jumps to the
oldest history and End returns to the latest message.

Ordinary reads, workspace edits, patches, and validation run without repeated
approval prompts. Codex still receives a workspace-write filesystem sandbox:
outside-workspace writes remain blocked. If TTED is itself running inside a
Codex permission profile, it reuses that outer boundary instead of nesting a
second Bubblewrap sandbox that the container may not permit.

Controls:

- **Stop** interrupts the current task.
- **Retry** sends the last request again.
- **New** starts a fresh conversation.
- **Clear** clears the visible transcript.
- **Diff** opens the current task's changes.
- **Accept** keeps the changes.
- **Revert** restores the pre-task files when they have not been edited again.

The panel does not lock the editor. Click the document or press Tab to return
to normal editing while chat stays open. Press Ctrl+G or click the prompt area
to focus chat again.

When you send a request, TTED synchronizes changed, named buffers so Codex sees
your latest text. An untitled buffer gets the normal filename popup first. You
can keep editing while Codex works; if both of you change the same file, TTED
shows its external-change choice instead of overwriting either version. Revert
will not overwrite a file that changed again after Codex finished.

The local structured API in [AGENT_API.md](AGENT_API.md) remains available for
advanced users and future agent providers, but it is not required for ordinary
Codex chat.
