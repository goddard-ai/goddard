//! Cheap immutable snapshots of transcript sequences. The tree shares its
//! structure, and each payload copies only when that payload is mutated.
//! Empty catalog projections allocate nothing; JSON remains an ordinary array.

use std::fmt;
use std::ops::{Deref, DerefMut, Index, IndexMut, RangeBounds};
use std::sync::Arc;

use imbl::Vector;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub struct SharedValue<T>(Arc<T>);

impl<T> From<T> for SharedValue<T> {
    fn from(value: T) -> Self {
        Self(Arc::new(value))
    }
}

impl<T> Clone for SharedValue<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> Deref for SharedValue<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for SharedValue<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

pub struct SharedHistory<T>(Option<Arc<Vector<SharedValue<T>>>>);

impl<T> Default for SharedHistory<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> Clone for SharedHistory<T> {
    fn clone(&self) -> Self {
        Self(self.0.as_ref().map(Arc::clone))
    }
}

type RawIter<'a, T> = <&'a Vector<SharedValue<T>> as IntoIterator>::IntoIter;
type RawIterMut<'a, T> = <&'a mut Vector<SharedValue<T>> as IntoIterator>::IntoIter;
type RawIntoIter<T> = <Vector<SharedValue<T>> as IntoIterator>::IntoIter;

pub struct Iter<'a, T> {
    inner: Option<RawIter<'a, T>>,
    remaining: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        let value = self.inner.as_mut()?.next()?;
        self.remaining -= 1;
        Some(value)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let value = self.inner.as_mut()?.next_back()?;
        self.remaining -= 1;
        Some(value)
    }
}
impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> std::iter::FusedIterator for Iter<'_, T> {}

pub struct IterMut<'a, T> {
    inner: Option<RawIterMut<'a, T>>,
    remaining: usize,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut SharedValue<T>;
    fn next(&mut self) -> Option<Self::Item> {
        let value = self.inner.as_mut()?.next()?;
        self.remaining -= 1;
        Some(value)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<T> DoubleEndedIterator for IterMut<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let value = self.inner.as_mut()?.next_back()?;
        self.remaining -= 1;
        Some(value)
    }
}
impl<T> ExactSizeIterator for IterMut<'_, T> {}
impl<T> std::iter::FusedIterator for IterMut<'_, T> {}

impl<T> SharedHistory<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |data| data.len())
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        self.0.as_ref()?.get(index).map(|value| &**value)
    }
    pub fn first(&self) -> Option<&T> {
        self.get(0)
    }
    pub fn last(&self) -> Option<&T> {
        self.get(self.len().checked_sub(1)?)
    }
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            inner: self.0.as_ref().map(|data| data.iter()),
            remaining: self.len(),
        }
    }
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let remaining = self.len();
        IterMut {
            inner: self.0.as_mut().map(|data| Arc::make_mut(data).iter_mut()),
            remaining,
        }
    }
    fn data_mut(&mut self) -> &mut Vector<SharedValue<T>> {
        Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vector::new())))
    }
    pub fn push(&mut self, value: T) {
        self.data_mut().push_back(SharedValue(Arc::new(value)));
    }
    pub fn insert(&mut self, index: usize, value: T) {
        self.data_mut().insert(index, SharedValue(Arc::new(value)));
    }
    pub fn clear(&mut self) {
        self.0 = None;
    }
    pub fn truncate(&mut self, len: usize) {
        if len == 0 {
            self.clear();
        } else if len < self.len() {
            self.data_mut().truncate(len);
        }
    }
    pub fn split_off(&mut self, index: usize) -> Self {
        assert!(index <= self.len(), "split index exceeds history length");
        if index == 0 {
            return std::mem::take(self);
        }
        if index == self.len() {
            return Self::new();
        }
        Self(Some(Arc::new(self.data_mut().split_off(index))))
    }
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let range = self.bounds(range);
        let mut copy = self.clone();
        copy.truncate(range.end);
        copy.split_off(range.start)
    }
    /// Borrow only the requested range, without flattening or copying the tree.
    pub fn range(
        &self,
        range: impl RangeBounds<usize>,
    ) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.bounds(range).map(|index| &self[index])
    }
    fn bounds(&self, range: impl RangeBounds<usize>) -> std::ops::Range<usize> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => {
                start.checked_add(1).expect("history range overflow")
            }
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end.checked_add(1).expect("history range overflow"),
            std::ops::Bound::Excluded(&end) => end,
            std::ops::Bound::Unbounded => self.len(),
        };
        assert!(
            start <= end && end <= self.len(),
            "history range out of bounds"
        );
        start..end
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        if let Some(data) = self.0.as_mut() {
            Arc::make_mut(data).retain(|value| keep(value));
        }
    }
    pub fn append(&mut self, other: &mut Self) {
        if let Some(data) = other.0.take() {
            self.data_mut().append(Arc::unwrap_or_clone(data));
        }
    }
    pub fn sort_by_key<K: Ord>(&mut self, mut key: impl FnMut(&T) -> K) {
        let Some(data) = &self.0 else { return };
        if data.len() < 2 {
            return;
        }
        // Queue items with equal timestamps retain their acceptance order.
        // Sort shared handles with Vec's stable sort, never clone payloads.
        let mut sorted = data.iter().cloned().collect::<Vec<_>>();
        sorted.sort_by_key(|value| key(value));
        self.0 = Some(Arc::new(sorted.into_iter().collect()));
    }
}

impl<T: Clone> SharedHistory<T> {
    pub fn get_mut(&mut self, index: usize) -> Option<&mut SharedValue<T>> {
        self.0
            .as_mut()
            .and_then(|data| Arc::make_mut(data).get_mut(index))
    }
    pub fn first_mut(&mut self) -> Option<&mut SharedValue<T>> {
        self.get_mut(0)
    }
    pub fn last_mut(&mut self) -> Option<&mut SharedValue<T>> {
        self.get_mut(self.len().checked_sub(1)?)
    }
    pub fn pop(&mut self) -> Option<T> {
        self.0.as_mut()?.as_ref().back()?;
        self.data_mut()
            .pop_back()
            .map(|value| Arc::unwrap_or_clone(value.0))
    }
    pub fn remove(&mut self, index: usize) -> T {
        Arc::unwrap_or_clone(self.data_mut().remove(index).0)
    }
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }
}

impl<T> Index<usize> for SharedHistory<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        self.get(index).expect("history index out of bounds")
    }
}
impl<T: Clone> IndexMut<usize> for SharedHistory<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.get_mut(index).expect("history index out of bounds")
    }
}

impl<'a, T> IntoIterator for &'a SharedHistory<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, T> IntoIterator for &'a mut SharedHistory<T> {
    type Item = &'a mut SharedValue<T>;
    type IntoIter = IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
impl<T: Clone> IntoIterator for SharedHistory<T> {
    type Item = T;
    type IntoIter = std::iter::Map<RawIntoIter<T>, fn(SharedValue<T>) -> T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0
            .map(Arc::unwrap_or_clone)
            .unwrap_or_default()
            .into_iter()
            .map(|value| Arc::unwrap_or_clone(value.0))
    }
}
impl<T> FromIterator<T> for SharedHistory<T> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        let data: Vector<_> = values
            .into_iter()
            .map(|value| SharedValue(Arc::new(value)))
            .collect();
        if data.is_empty() {
            Self::new()
        } else {
            Self(Some(Arc::new(data)))
        }
    }
}
impl<T> From<Vec<T>> for SharedHistory<T> {
    fn from(values: Vec<T>) -> Self {
        values.into_iter().collect()
    }
}
impl<T> Extend<T> for SharedHistory<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, values: I) {
        for value in values {
            self.push(value);
        }
    }
}
impl<T: fmt::Debug> fmt::Debug for SharedHistory<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}
impl<T: PartialEq> PartialEq for SharedHistory<T> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}
impl<T: Eq> Eq for SharedHistory<T> {}
impl<T: PartialEq> PartialEq<Vec<T>> for SharedHistory<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        self.iter().eq(other.iter())
    }
}
impl<T: Serialize> Serialize for SharedHistory<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for SharedHistory<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Serialize, Deserialize)]
    struct Payload {
        text: String,
        #[serde(skip)]
        clones: Arc<AtomicUsize>,
    }

    impl Clone for Payload {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::Relaxed);
            Self {
                text: self.text.clone(),
                clones: Arc::clone(&self.clones),
            }
        }
    }

    #[test]
    fn snapshots_share_payloads_and_isolate_worker_reads_from_live_edits() {
        let clones = Arc::new(AtomicUsize::new(0));
        let mut live: SharedHistory<_> = (0..4096)
            .map(|index| Payload {
                text: index.to_string(),
                clones: Arc::clone(&clones),
            })
            .collect();
        let snapshot = live.clone();
        assert_eq!(clones.load(Ordering::Relaxed), 0);
        let (resume, ready) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            ready.recv().unwrap();
            let tail = snapshot.slice(2048..);
            serde_json::to_vec(&tail).unwrap()
        });
        // Searching mutable candidates must not copy every preceding payload.
        live.iter_mut()
            .find(|value| value.text == "2048")
            .unwrap()
            .text
            .push_str(" edited");
        live.push(Payload {
            text: "appended".into(),
            clones: Arc::clone(&clones),
        });
        assert_eq!(clones.load(Ordering::Relaxed), 1);
        resume.send(()).unwrap();
        let bytes = worker.join().unwrap();
        let restored: Vec<Payload> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.len(), 2048);
        assert_eq!(restored[0].text, "2048");
        assert_eq!(restored.last().unwrap().text, "4095");
        assert_eq!(live[2048].text, "2048 edited");
        let restored: SharedHistory<Payload> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            serde_json::to_vec(&restored).unwrap(),
            bytes,
            "wire shape stays an array"
        );
    }
}
