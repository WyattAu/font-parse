//! A table view must not outlive the buffer it borrows.

use font_parse::FontRef;

fn main() {
    let data = vec![0u8; 64];
    let font = {
        let tmp = data.clone();
        font_parse::OwnedFont::from_sfnt(tmp).unwrap()
    };
    // `font` owns its buffer, so this is fine — force the borrow of the
    // short-lived `data` instead:
    let view = {
        let data2 = data.clone();
        FontRef::new(&data2).unwrap()
    }; // data2 dropped here
    let _head = view.head().unwrap(); //~ ERROR: `data2` does not live long enough
}
