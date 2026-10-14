pub mod engine;
pub mod ocr;
#[cfg(feature = "gui")]
pub mod ui;
pub mod worker;
pub const THIRD_PARTY_LICENSES: &str = concat!(
    "FLATTENER SOFTWARE LICENSE\n\n",
    include_str!("../LICENSE"),
    "\n\n",
    include_str!("../docs/THIRD_PARTY_LICENSES.txt")
);
