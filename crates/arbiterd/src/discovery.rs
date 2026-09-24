use rand::Rng;

pub(crate) fn new_token() -> String {
    let bytes: [u8; 24] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
