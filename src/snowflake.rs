use std::time::{SystemTime, UNIX_EPOCH};

const EPOCH: u64 = 1740000000000;
const MACHINE_ID_BITS: u8 = 10;
const SEQUENCE_BITS: u8 = 12;

pub struct SnowflakeGenerator {
    machine_id: u16,
    sequence: u16,
    last_timestamp: u64,
}

impl SnowflakeGenerator {
    pub fn new(machine_id: u16) -> Self {
        Self {
            machine_id,
            sequence: 0,
            last_timestamp: 0,
        }
    }

    pub fn generate(&mut self) -> i64 {
        let mut timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before UNIX_EPOCH")
            .as_millis() as u64;

        if timestamp < self.last_timestamp {
            panic!(
                "Clock moved backwards: last_timestamp={}, now={}",
                self.last_timestamp, timestamp
            );
        }

        if timestamp == self.last_timestamp {
            self.sequence += 1;

            if self.sequence > ((1u16 << SEQUENCE_BITS) - 1) {
                loop {
                    timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("System time is before UNIX_EPOCH")
                        .as_millis() as u64;
                    if timestamp > self.last_timestamp {
                        break;
                    }
                }
                self.sequence = 0;
            }
        } else {
            self.sequence = 0;
        }

        self.last_timestamp = timestamp;

        let id = ((timestamp - EPOCH) << (MACHINE_ID_BITS + SEQUENCE_BITS))
            | ((self.machine_id as u64) << SEQUENCE_BITS)
            | (self.sequence as u64);

        id as i64
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn generate_returns_positive_id() {
        let mut sf = SnowflakeGenerator::new(1);
        let id = sf.generate();
        assert!(id > 0, "Snowflake ID should be positive, got {id}");
    }

    #[test]
    fn generate_returns_unique_ids() {
        let mut sf = SnowflakeGenerator::new(1);
        let mut ids = HashSet::new();

        for _ in 0..1000 {
            let id = sf.generate();
            assert!(ids.insert(id), "Duplicate ID detected: {id}");
        }
    }

    #[test]
    fn generate_returns_increasing_ids() {
        let mut sf = SnowflakeGenerator::new(1);
        let mut prev = sf.generate();

        for _ in 0..100 {
            let id = sf.generate();
            assert!(
                id > prev,
                "IDs should be monotonically increasing: {prev} >= {id}"
            );
            prev = id;
        }
    }

    #[test]
    #[should_panic(expected = "Clock moved backwards")]
    fn generate_panics_on_clock_rollback() {
        let mut sf = SnowflakeGenerator::new(1);
        // 未来のタイムスタンプを直接設定して巻き戻りをシミュレート
        sf.last_timestamp = u64::MAX;
        sf.generate();
    }

    #[test]
    fn different_machine_ids_produce_different_ids() {
        let mut gen_a = SnowflakeGenerator::new(1);
        let mut gen_b = SnowflakeGenerator::new(2);

        let id_a = gen_a.generate();
        let id_b = gen_b.generate();

        assert_ne!(
            id_a, id_b,
            "Different machine IDs should produce different IDs"
        );
    }
}
