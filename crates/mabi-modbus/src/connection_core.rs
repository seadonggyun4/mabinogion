use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

/// Shared shard routing logic for connection-oriented data structures.
#[derive(Debug, Clone)]
pub struct ShardRouter {
    shard_count: usize,
    shard_mask: Option<usize>,
}

impl ShardRouter {
    pub fn new(shard_count: usize) -> Self {
        assert!(shard_count > 0, "shard_count must be greater than zero");
        let shard_mask = shard_count.is_power_of_two().then_some(shard_count - 1);
        Self {
            shard_count,
            shard_mask,
        }
    }

    #[cfg_attr(not(feature = "experimental-scaling"), allow(dead_code))]
    #[inline]
    pub fn shard_count(&self) -> usize {
        self.shard_count
    }

    #[inline]
    pub fn index_for_id(&self, id: u64) -> usize {
        if let Some(mask) = self.shard_mask {
            (id as usize) & mask
        } else {
            (id as usize) % self.shard_count
        }
    }

    #[inline]
    pub fn index_for_addr(&self, addr: &SocketAddr) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        addr.hash(&mut hasher);
        let hash = hasher.finish();

        if let Some(mask) = self.shard_mask {
            (hash as usize) & mask
        } else {
            (hash as usize) % self.shard_count
        }
    }
}

/// Compact ordered set for Modbus unit identifiers.
#[derive(Debug, Clone, Default)]
pub struct UnitIdSet {
    bitmap: [u64; 4],
    ordered: Vec<u8>,
}

impl UnitIdSet {
    #[inline]
    pub fn record(&mut self, unit_id: u8) -> bool {
        let bucket = (unit_id / 64) as usize;
        let bit = 1u64 << (unit_id % 64);
        if self.bitmap[bucket] & bit != 0 {
            return false;
        }

        self.bitmap[bucket] |= bit;
        self.ordered.push(unit_id);
        true
    }

    #[inline]
    pub fn snapshot(&self) -> Vec<u8> {
        self.ordered.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_uses_power_of_two_fast_path() {
        let router = ShardRouter::new(8);
        assert_eq!(router.index_for_id(0), 0);
        assert_eq!(router.index_for_id(7), 7);
        assert_eq!(router.index_for_id(8), 0);
    }

    #[test]
    fn unit_id_set_preserves_first_seen_order() {
        let mut set = UnitIdSet::default();
        assert!(set.record(7));
        assert!(set.record(3));
        assert!(!set.record(7));
        assert!(set.record(255));
        assert_eq!(set.snapshot(), vec![7, 3, 255]);
    }
}
