#[derive(Debug, Default, PartialEq, Eq, Clone, Copy, Serializable, Deserializable)]
pub struct Services(u64);

impl From<Services> for u64 {
    fn from(s: Services) -> Self {
        s.0
    }
}

impl From<u64> for Services {
    fn from(v: u64) -> Self {
        Services(v)
    }
}

impl Services {
    pub fn network(&self) -> bool {
        self.bit_at(0)
    }

    pub fn with_network(mut self, v: bool) -> Self {
        self.set_bit(0, v);
        self
    }

    pub fn includes(&self, other: &Self) -> bool {
        self.0 & other.0 == other.0
    }

    fn set_bit(&mut self, bit: usize, bit_value: bool) {
        if bit_value {
            self.0 |= 1 << bit
        } else {
            self.0 &= !(1 << bit)
        }
    }

    fn bit_at(&self, bit: usize) -> bool {
        self.0 & (1 << bit) != 0
    }
}

#[cfg(test)]
mod test {
    use super::Services;

    #[test]
    fn test_serivces_includes() {
        let s1 = Services::default();
        let s2 = Services::default();

        assert!(s1.includes(&s2));
        assert!(s2.includes(&s1));
    }
}
