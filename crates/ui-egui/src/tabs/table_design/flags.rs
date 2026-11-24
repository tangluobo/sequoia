//! Preserve legacy clipboard flag spelling without six heap strings per normal row.
#[derive(Clone, Default)]
pub(super) struct Flags {
    unset: u8,
    other: Option<Box<[String; 6]>>,
}
impl Flags {
    pub fn empty() -> Self {
        Self {
            unset: 0x3f,
            other: None,
        }
    }
    pub fn from_strings(values: [String; 6]) -> Self {
        let mut unset = 0;
        for (bit, value) in values.iter().enumerate() {
            if value.is_empty() {
                unset |= 1 << bit;
            }
        }
        let other = values
            .iter()
            .any(|value| !["", "是", "否"].contains(&value.as_str()))
            .then(|| Box::new(values));
        Self { unset, other }
    }
    pub fn text(&self, value: bool, bit: usize) -> &str {
        if let Some(other) = &self.other {
            &other[bit]
        } else if value {
            "是"
        } else if self.unset & (1u8 << bit) != 0 {
            ""
        } else {
            "否"
        }
    }
    pub fn set(&mut self, bit: usize, value: bool) {
        self.unset &= !(1u8 << bit);
        if let Some(other) = &mut self.other {
            other[bit] = if value { "是" } else { "否" }.into();
        }
    }
}
