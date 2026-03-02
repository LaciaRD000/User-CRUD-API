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
