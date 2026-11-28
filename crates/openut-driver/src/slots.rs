//! The stable, provider-owned storage behind tracker objects.

use std::sync::Arc;

use openut_bridge::MAX_DEVICES;

pub struct Slots<T> {
    #[expect(
        clippy::redundant_allocation,
        reason = "the design keeps a boxed device; shared handles outlive the storage lock"
    )]
    entries: [Option<Arc<Box<T>>>; MAX_DEVICES],
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
        }
    }
}

impl<T> Slots<T> {
    /// A slot is allocated once. The shared handle allows calls after releasing
    /// the provider's storage lock, while the box keeps the object at one address.
    pub fn insert(&mut self, index: usize, value: T) -> Option<Arc<Box<T>>> {
        let entry = self.entries.get_mut(index)?;
        if entry.is_some() {
            return None;
        }
        let handle = Arc::new(Box::new(value));
        *entry = Some(Arc::clone(&handle));
        Some(handle)
    }

    pub fn get(&self, index: usize) -> Option<Arc<Box<T>>> {
        self.entries.get(index)?.clone()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<Box<T>>> {
        self.entries.iter().flatten()
    }

    /// Release all provider ownership after its permits and receiver are done.
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct CountDrop(Arc<AtomicUsize>);

    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn boxed_objects_stay_at_one_address_until_cleanup() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut slots = Slots::default();
        let first = slots.insert(0, CountDrop(Arc::clone(&drops))).unwrap();
        let first_address = std::ptr::from_ref(&**first);
        drop(first);
        for slot in 1..MAX_DEVICES {
            slots.insert(slot, CountDrop(Arc::clone(&drops))).unwrap();
        }
        let mut moved = slots;
        let after_move = moved.get(0).unwrap();
        assert_eq!(std::ptr::from_ref(&**after_move), first_address);
        drop(after_move);
        assert_eq!(moved.iter().count(), MAX_DEVICES);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        moved.clear();
        assert_eq!(drops.load(Ordering::Relaxed), MAX_DEVICES);
        assert!(moved.get(0).is_none());
    }

    #[test]
    fn duplicate_and_out_of_range_slots_cannot_replace_boxes() {
        let mut slots = Slots::default();
        slots.insert(0, String::from("original")).unwrap();
        assert!(slots.insert(0, String::from("second")).is_none());
        assert!(slots.insert(MAX_DEVICES, String::from("extra")).is_none());
        assert_eq!(slots.get(0).unwrap().as_str(), "original");
    }
}
