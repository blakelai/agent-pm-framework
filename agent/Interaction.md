---
type: "Reference"
title: "Terminal interaction contract"
language: "en"
---

# Terminal interaction contract

The user-approved interface separates project browsing and Agent chat. It preserves the selected document, category position, unfinished input and active operation when switching modes. This is PM Agent's own contract, informed by the requested Codex CLI interaction style; it does not claim exact shortcut parity with every Codex release.

## Modes and navigation

F2 or a click on the top mode labels switches modes. Browse starts with the document list focused; Chat starts with the composer focused. F1 opens help. Ctrl+Q exits only when idle; Browse also accepts q. Chat accepts `/exit` (or `/quit`) and Ctrl+D with an exactly empty composer, no attachments and no pending clipboard read. Ctrl+D does not discard a nonempty draft and has no exit action in explicit keyboard transcript focus.

Browse uses Alt+1–9 for the first nine categories, Alt+0 for the complete eleven-category menu, and Alt+Left/Right to cycle. Category labels are clickable; overflow remains available through More. F6 switches emoji on/off. Type-based document icons retain readable titles.

Selecting a document previews it. Enter moves focus to its content. Esc restores the document list and focus. F3 collapses/expands the list; F4 maximizes/restores the document. Right-click opens document actions: desktop opening, properties/provenance, links, reference in chat and list visibility. P opens properties, L opens numbered links, O opens the original and R refreshes from disk.

[Automatic refresh](Automatic-Refresh.md) polls local knowledge files in the background every two seconds. It preserves selected paths, reading positions and chat drafts. Snapshot menus and scheduling pickers close when their inputs change; form drafts remain intact and reviewed previews become invalid. A visible error replaces unavailable data until the files can be read again.

E opens the [structured editor](Editing.md) for a supported project record. N starts a PBI, risk or decision. The editor consumes its own keyboard/mouse input; Ctrl+S reviews, then saves on a separate action. Esc in a field reverts that field; Esc in preview returns to the form. Ctrl+D closes with confirmation when changes would be discarded. Editing is available only when no operation is running.

## Reading and Markdown

F8 or Impact / compare in a document's context menu opens the [read-only inspector](Inspection.md). Its three tabs show reference chains, an explicitly selected baseline diff and pinned-source versus latest-snapshot differences. Enter compares a selected baseline, O reads a candidate, R revalidates, F4 maximizes and Esc returns focus before closing. Mouse navigation and a detail scrollbar are available. External project changes clear results and require another recheck; no source sync or semantic invalidation is automatic.

F7 or Delivery opens [Change, UAT, Release and Benefit workflows](Delivery-Workflows.md). Select a tab with 1–4, Alt+1–4 or a click; N creates a draft and E/right-click edits a supported record. Enter focuses details, Esc returns to the record list, and another Esc returns to Browse. Mouse scrolling and the detail scrollbar remain available; F6 toggles emoji. The same preview/save and discard controls apply. Current checks never create decisions or release events.

F5 or the Planning button in Browse opens [interactive scheduling](Planning-View.md). Its Schedule, Capacity and Signals tabs support keyboard and mouse navigation. M/right-click proposes a PBI move and opens a reviewed editor; selecting an iteration alone never saves it. Esc closes the target picker first, then returns to Browse. Existing form discard controls apply to any proposed change. R refreshes the planning snapshot immediately; P/I/A edit People, Iterations and the forecast date.

The browser and transcript share a terminal Markdown renderer: headings, emphasis, lists, task markers, quotes, fenced/inline code, tables and links. Obsidian links navigate to an unambiguous local document. Links in code remain literal. Images and diagrams are not visually rendered; linked originals can be opened using the desktop application. Metadata is shown through Properties instead of raw frontmatter.

The right scrollbar uses rendered rows after wrapping. Wheel events operate on the pointed pane; scrollbar clicks and drags change its position. Clicking or scrolling the conversation keeps the composer focused and preserves its cursor. This includes right-clicks and scrollbar dragging; they do not open a blocking menu. Chat follows the latest output by default. Page Up, wheel-up or moving the scrollbar back enters scroll mode and pauses following independently of keyboard focus. Ordinary clicks preserve the current following state; an upward scroll with no earlier content keeps following enabled. New streamed text preserves the reading position, and typing remains available immediately. A centered `Back to bottom · Esc` hint appears below the conversation while in scroll mode, above the execution progress row. Esc, clicking the hint or reaching the bottom via Page Down, wheel-down or the scrollbar resumes following. These actions retain the active Agent request and input draft. Shift+Tab explicitly selects keyboard transcript navigation; Tab or Esc returns to the composer.

## Recovery

Browse offers F11 Recovery, a top-bar action and a document context-menu entry. The [Recovery panel](Backup-and-Restore.md#tui-recovery-panel) uses Alt+1/2/3 or mouse tabs, directly editable absolute paths and a session checkbox. Ctrl+S reviews, then a separate Ctrl+S executes a write. Esc/right-click returns or requests cancellation; the panel waits for the actual background result. It keeps the current Vault and unsent Chat input intact.

## Chat input and cancellation

The transcript and composer are borderless. Only the composer has a gray background. Input is immediately editable; Enter submits. Alt+Enter, supported Shift+Enter and Ctrl+J insert a newline. Bracketed multiline paste inserts text without submitting. Arrow/Home/End and grapheme-aware deletion edit the draft; clicking the composer places the cursor. Ctrl+W deletes preceding whitespace followed by one whitespace-delimited word before the cursor, preserving text after it. Combining characters and Emoji remain intact; punctuation and unspaced Chinese belong to the same word. The input grows to a bounded height and then scrolls internally.

Up at the first logical input line recalls a previous submitted input; Down at the last line advances through history and restores the unfinished draft. Within a multiline draft, Up/Down still move between lines. Recall is local to the running interface (up to 200 inputs); loading a session also makes its saved user messages available.

While an operation runs, the next draft remains editable but cannot be submitted automatically. Drafts survive mode changes. Ctrl+C clears the draft and pending image attachments without cancelling an operation or exiting.

When a tool approval picker is visible, Esc immediately selects Deny and stop, even while reading older output. Otherwise Esc is handled by exactly one layer:

1. An open action menu handles Esc by closing itself.
2. Scroll mode handles Esc by jumping to the latest output, resuming following and returning focus to the composer. Any open command/Skill suggestions are dismissed by the same action; the Agent request continues.
3. While following, an open command/Skill suggestion handles Esc by closing itself. Explicit transcript focus handles Esc by returning to the latest output and composer.
4. Composer focus while following handles a separate Esc by requesting cancellation of an active operation.
5. An idle composer leaves its draft and mode unchanged.

Reported repeat key events do not trigger another action. Keyboard enhancement reporting is enabled where supported. Legacy terminals that do not distinguish key repeats cannot establish physical key release; verify that behavior in the terminal used for deployment.

Exit during active Chat work requests cancellation and waits for the recorded outcome before restoring the terminal. A native `/pm` worker cannot be forcibly rolled back: the interface waits for it to finish. If the background operation fails to return a recorded outcome, the error remains visible and a separate idle exit is available.

Cancellation shows a pending state until the runtime records an outcome. A tool with an uncertain outcome still requires reconciliation; returning focus never implies cancellation, rollback or tool completion.

## Clipboard images

Ctrl+V in the composer reads the system clipboard once on a background worker. A decoded image becomes a pending PNG attachment, with its number and dimensions shown above the text inside the gray composer. If no image is available, readable clipboard text is inserted at the cursor without submission. The application never polls or overwrites the clipboard. It does not fetch remote image URLs or interpret clipboard filenames as image bytes.

Click an attachment chip to remove it, or use Ctrl+X to remove the last pending image. Ctrl+C clears text and images and discards a delayed clipboard result. Enter waits for a pending read to finish; it then sends the text and images together, including an image-only message. Up to four images may be attached to one message. Each normalized PNG is limited to 8 MiB and 32 megapixels; the total raw PNG payload in a request is limited to 20 MiB. These byte limits are unrelated to service-reported Context usage. Pixels are not resized to meet a limit. Very narrow terminals may hide chips; Ctrl+X and Ctrl+C remain available.

Local commands retain pending attachments; `/answer QUESTION-ID` can send an image without additional text. Attachments are removed from the composer only after the runtime accepts that submitted input. Newer images pasted while it starts remain pending for the next message. An input rejection preserves attachments and restores its text when the composer is empty. If a provider later rejects the request, the accepted input and images remain in the session; `/continue` retains them. There is no automatic retry that silently omits images.

Submitted PNGs use ordinary `01_Sources/Assets` storage and source notes, so the existing Vault backup and Materials workflows include them. Sessions save local references, dimensions and SHA-256 hashes, not base64. Every request rechecks the asset before constructing the protocol payload. Missing, changed or out-of-scope images block sending; no image is silently dropped. Session replay displays attachment labels, not pixels. Unsent attachments are held in memory and are not restored after exit.

Pasting is not visual review, OCR, source approval or clarification resolution. New source records remain pending review; reusing an existing image does not change its review state. The draft grounding gate still requires supported, reviewable text evidence for claims. Image-dependent facts must pass the existing material review and evidence workflow before becoming project assertions.

The selected service and model must accept image inputs. Responses uses `input_image` and Chat Completions uses `image_url`, with PNG data URLs, following the official [image input formats](https://developers.openai.com/api/docs/guides/images-vision?api-mode=chat), checked on 2026-11-01. Protocol fixture tests do not establish a particular model's visual interpretation accuracy.

Native clipboard access uses [arboard](https://docs.rs/arboard/latest/arboard/): macOS requires access to the local desktop clipboard; Linux uses X11 or a Wayland compositor supporting the data-control protocol. Headless/SSH environments and unsupported clipboard formats may be unavailable; failures appear in the status line and preserve the draft. A terminal that intercepts Ctrl+V must be configured to forward the key. Text remains available through ordinary terminal paste. Desktop clipboard APIs decode images before the application checks pixel limits.

## Tool approval composer

A durable WaitingApproval session replaces the text/image composer with a decision picker. It summarizes the actual pending action, why permission is needed, its arguments and the scope of reusable choices. The choices are Allow once, Allow for this conversation, Always allow (global settings), and Deny and stop. Up/Down or 1–4 selects a row; Enter or a mouse click confirms. Esc immediately denies and stops the response. Paste and text-editing keys do not alter the preserved draft while the picker is open. Page Up/Down and Ctrl+T can inspect the conversation; F2 preserves the pending request while browsing.

A selection binds the saved conversation and exact pending call. A changed pending call requires review again. A server configuration or schema change while waiting also requires another review before permission is saved. Successful permission decisions resume the Agent automatically; denials use only local saved state, cancel all remaining calls from that response and send no model request. The normal composer returns with its text, cursor and attachments intact. Errors in saving global settings restore the pending picker. Loading a waiting session restores the picker as well.

Conversation permissions are persisted with that session. Global permissions live in the separate user settings file described in [application setup](README.md#tool-approval-choices). Both apply to the current tool with different future arguments; PM permissions distinguish subcommands and MCP permissions bind server configuration and parameter schema. All existing execution and clarification checks still apply.

## Command and Skill suggestions

Type `/` at the start of the composer to open command suggestions; continued typing filters them. `/stage ` offers workflow stages and `/pm ` offers the native command catalog. Type `$` as a leading activation token to filter valid project Skills by name. Multiple leading Skills are supported. A `$` inside ordinary prose does not activate a Skill or open its picker.

Use Up/Down or the wheel to select, then Tab, Enter or a left-click to insert the candidate. Selection only edits the draft; a separate Enter submits it. Exact `/exit` and `/quit` commands exit with one Enter. Esc dismisses suggestions without cancelling the Agent. Empty results do not execute partial text. The popup adapts to the available height; on terminals with fewer than three available transcript rows, enlarge the window to see it or dismiss it with Esc.

Skill discovery refreshes when entering a new `$` completion context. Invalid entries remain diagnosable through `/skills`; runtime validation checks the selected files again before any model request. Suggestions do not grant new tools or bypass approvals. Command arguments such as session IDs and paths are entered manually.

## Conversation and execution details

User messages and ordinary Agent replies remain expanded and use the shared Markdown renderer, with separate labels and spacing. Thinking, tool calls, diagnostics and activity notices start collapsed with at most three content rows, including the heading, after terminal-width wrapping. Previews omit blank rows and indicate shortened content with an ellipsis. Public thinking previews follow the latest two rendered rows; other previews show the first two detail rows. Left-click a heading to expand/collapse that entry; Ctrl+T expands all details if any are collapsed, otherwise collapses them. New entries follow that preference. These actions never invoke a tool, change an approval or cancel execution.

Tool headings distinguish requested/running/pending calls and recorded result status. A call updates its existing entry when a result arrives. Built-in tools have specific action labels and previews: document searches show the query, returned count and sample titles, source listings show scope/pagination, reads show line counts and content, and drafts distinguish new saves from updates while retaining the human-review requirement. Searches at the 60-result cap mark the total as unknown. PM subcommands use operation-specific labels. Expanded tools show readable request/result fields, document or report Markdown, and literal metadata paths instead of raw response JSON. External tools use a generic field layout; approval arguments remain available. Complete original Agent calls/results stay in saved history, with explicit display-shortening notices for bounded projections. Local `/pm` operations use the same presentation.

An error or blocked result does not become a success just because a call returned. Attention states lead the heading so long paths cannot hide them. Approval requests, clarification prompts and errors retain a visible heading while their details are collapsed. Scrolling pauses following while keeping input available; Esc resumes following at the latest output.

A progress row is pinned above the composer independently of transcript scrolling. It uses public runtime phases and tool actions to show preparation, service waiting, source-support checking, tool execution or response writing. The activity marker advances at 10 Hz even without incoming events; elapsed seconds and the Esc cancellation hint appear when space permits and Esc can cancel directly. Scroll mode shows its own return-to-bottom hint and hides the cancellation hint. Clarification, approval and terminal outcomes stop the animation. Cancellation shows that the operation outcome is still pending; late events cannot replace that status, and uncertain outcomes remain explicit until reconciled. Progress is ephemeral, makes no additional model requests, and neither infers model intent nor estimates completion percentages. Fixed labels follow the existing English UI, independently of the model's response language.

Thinking is displayed only when the service supplies it: Responses public `response.reasoning_summary_text.delta` events (or completed output summaries), and compatible Chat Completions `delta.reasoning_content` / `delta.reasoning` string fields. No thinking is inferred from ordinary answer text, tool calls or elapsed time. Opaque encrypted continuation items are retained for Responses history but never rendered. The internal grounding reviewer remains isolated from the conversation.

Completed compatible-API reasoning is saved separately from the wire conversation; Responses summaries are replayed from their original output items. Old sessions without reasoning remain readable and do not acquire invented summaries. Loading a session reconstructs message roles, calls, results and pending status. Partial interrupted streams are display-only; the last incomplete provider response is not added to accepted history. The bounded transcript may omit older display text without truncating saved session history.

Failures use a red, collapsed diagnostic heading. Click it or use Ctrl+T for literal multiline details: operation, available paths/configuration/server identity, original causes and relevant next steps. File paths retain punctuation instead of being parsed as Markdown. Execution failures saved in the session reappear on resume; older generic errors do not acquire invented paths. Startup failures before input acceptance remain current-conversation diagnostics. See [Chat troubleshooting](Troubleshooting.md) for offline checks and recovery directions.

The interaction draws on the official [Codex CLI command and history guidance](https://learn.chatgpt.com/docs/developer-commands?surface=cli). This application's Ctrl+C clear, Esc cancellation and Ctrl+T details follow the user's explicit contract. Provider field handling follows [Responses streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events) and [vLLM reasoning outputs](https://docs.vllm.ai/en/latest/features/reasoning_outputs/), checked on 2026-11-01. Support for these optional fields does not guarantee that every configured service/model returns them.

## Status and usage

The line directly below input shows the configured model, runtime state, short session ID and the latest service-reported input token count. Click it or use /status for the full ID, selected Skills and reported input/output/total fields. Available width can abbreviate the model.

Usage is parsed from the completed Responses response or Chat Completions usage chunk. Missing values remain "not provided"; zero appears only when reported as zero. No tokenizer, local estimate, model capacity assumption, accumulated-session approximation or inferred percentage is used. A new request clears the previous display; a response without usage does not reuse an earlier request's values.

Chat Completions profiles request `stream_options.include_usage` by default. Set `request_usage = false` inside a provider profile if the gateway rejects that optional request field. There is no automatic retry or protocol fallback. This opt-out still accepts usage when a service sends it.

`/budget` displays [session usage and an optional reported-token guard](Session-Limits.md) separately from Context. It is a local query that preserves input, model and session state. Normal Chat has no request-count cap, including for old configurations containing `max_steps` or `max_requests`. An explicitly enabled token guard can still produce `LimitReached` and preserve pending tools; `/continue` checks that guard again. Missing usage is never estimated or reset.

The [OpenAI streaming reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events) defines usage chunks separately from content deltas. Compatible services require their own live verification.

## Terminal compatibility

Mouse capture, bracketed paste and keyboard enhancements are restored on ordinary exit or unwind. Keep keyboard alternatives for mouse operations. Alt/Option combinations and Shift+Enter depend on terminal key configuration; Ctrl+J remains an explicit newline alternative. Emoji can be disabled when a font's cell widths differ.

Automated tests exercise events and terminal buffers, including small windows, Unicode input, focus/cancellation, mouse scrolling, Markdown and usage omissions. These checks do not certify every terminal, font, input method or provider.
