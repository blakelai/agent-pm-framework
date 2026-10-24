---
type: "Reference"
title: "Terminal interaction contract"
language: "en"
---

# Terminal interaction contract

The user-approved interface separates project browsing and Agent chat. It preserves the selected document, category position, unfinished input and active operation when switching modes. This is PM Agent's own contract, informed by the requested Codex CLI interaction style; it does not claim exact shortcut parity with every Codex release.

## Modes and navigation

F2 or a click on the top mode labels switches modes. Browse starts with the document list focused; Chat starts with the composer focused. F1 opens help. Ctrl+Q exits only when idle; Browse also accepts q and Chat accepts /quit.

Browse uses Alt+1–9 for the first nine categories, Alt+0 for the complete eleven-category menu, and Alt+Left/Right to cycle. Category labels are clickable; overflow remains available through More. F6 switches emoji on/off. Type-based document icons retain readable titles.

Selecting a document previews it. Enter moves focus to its content. Esc restores the document list and focus. F3 collapses/expands the list; F4 maximizes/restores the document. Right-click opens document actions: desktop opening, properties/provenance, links, reference in chat and list visibility. P opens properties, L opens numbered links, O opens the original and R refreshes from disk.

E opens the [structured editor](Editing.md) for a supported project record. N starts a PBI, risk or decision. The editor consumes its own keyboard/mouse input; Ctrl+S reviews, then saves on a separate action. Esc in a field reverts that field; Esc in preview returns to the form. Ctrl+D closes with confirmation when changes would be discarded. Editing is available only when no operation is running.

## Reading and Markdown

The browser and transcript share a terminal Markdown renderer: headings, emphasis, lists, task markers, quotes, fenced/inline code, tables and links. Obsidian links navigate to an unambiguous local document. Links in code remain literal. Images and diagrams are not visually rendered; linked originals can be opened using the desktop application. Metadata is shown through Properties instead of raw frontmatter.

The right scrollbar uses rendered rows after wrapping. Wheel events operate on the pointed pane; scrollbar clicks and drags change its position. Scrolling the conversation enters reading focus and pauses following new output. New streamed text does not pull the reader to the bottom.

## Chat input and cancellation

The transcript and composer are borderless. Only the composer has a gray background. Input is immediately editable; Enter submits. Alt+Enter, supported Shift+Enter and Ctrl+J insert a newline. Bracketed multiline paste inserts text without submitting. Arrow/Home/End and grapheme-aware deletion edit the draft; clicking the composer places the cursor. The input grows to a bounded height and then scrolls internally.

While an operation runs, the next draft remains editable but cannot be submitted automatically. Drafts survive mode changes. Ctrl+C clears the draft without cancelling an operation or exiting.

Esc is handled by exactly one layer:

1. An open menu handles Esc by closing itself.
2. Transcript focus handles Esc by jumping to the latest output, resuming following and returning focus to the composer.
3. Composer focus handles a separate Esc by requesting cancellation of an active operation.
4. An idle composer leaves its draft and mode unchanged.

Reported repeat key events do not trigger another action. Keyboard enhancement reporting is enabled where supported. Legacy terminals that do not distinguish key repeats cannot establish physical key release; verify that behavior in the terminal used for deployment.

Cancellation shows a pending state until the runtime records an outcome. A tool with an uncertain outcome still requires reconciliation; returning focus never implies cancellation, rollback or tool completion.

## Status and usage

The line directly below input shows the configured model, runtime state, short session ID and the latest service-reported input token count. Click it or use /status for the full ID, selected Skills and reported input/output/total fields. Available width can abbreviate the model.

Usage is parsed from the completed Responses response or Chat Completions usage chunk. Missing values remain "not provided"; zero appears only when reported as zero. No tokenizer, local estimate, model capacity assumption, accumulated-session approximation or inferred percentage is used. A new request clears the previous display; a response without usage does not reuse an earlier request's values.

Chat Completions profiles request `stream_options.include_usage` by default. Set `request_usage = false` inside a provider profile if the gateway rejects that optional request field. There is no automatic retry or protocol fallback. This opt-out still accepts usage when a service sends it.

The [OpenAI streaming reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events) defines usage chunks separately from content deltas. Compatible services require their own live verification.

## Terminal compatibility

Mouse capture, bracketed paste and keyboard enhancements are restored on ordinary exit or unwind. Keep keyboard alternatives for mouse operations. Alt/Option combinations and Shift+Enter depend on terminal key configuration; Ctrl+J remains an explicit newline alternative. Emoji can be disabled when a font's cell widths differ.

Automated tests exercise events and terminal buffers, including small windows, Unicode input, focus/cancellation, mouse scrolling, Markdown and usage omissions. These checks do not certify every terminal, font, input method or provider.
