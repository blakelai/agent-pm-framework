//! Explicit opt-in only: run under a disposable Xvfb server, never a user's desktop.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "Requires a disposable X11 clipboard: xvfb-run -a cargo test --test clipboard -- --ignored"]
fn isolated_x11_image_and_text_clipboard_roundtrip() {
    use pm_agent::chat_images::{self, ClipboardContent};
    use std::borrow::Cow;
    assert_eq!(
        std::env::var("PM_TEST_ISOLATED_CLIPBOARD").as_deref(),
        Ok("1")
    );
    let mut owner = arboard::Clipboard::new().unwrap();
    owner
        .set_image(arboard::ImageData {
            width: 2,
            height: 1,
            bytes: Cow::Owned(vec![255, 0, 0, 255, 0, 255, 0, 255]),
        })
        .unwrap();
    match chat_images::clipboard().unwrap() {
        ClipboardContent::Image(image) => assert_eq!((image.width, image.height), (2, 1)),
        ClipboardContent::Text(_) => panic!("Image paste became text"),
    }
    owner.set_text("Synthetic clipboard\n中文").unwrap();
    match chat_images::clipboard().unwrap() {
        ClipboardContent::Text(text) => assert_eq!(text, "Synthetic clipboard\n中文"),
        ClipboardContent::Image(_) => panic!("Old image survived clipboard replacement"),
    }
}
