pub fn used() -> u32 {
    helper() + 1
}

fn helper() -> u32 {
    41
}

#[allow(dead_code)]
fn orphan() -> u32 {
    0
}
