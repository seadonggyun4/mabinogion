//! BACnet File object implementation per ASHRAE 135, Clause 12.13.
//!
//! The File object represents a data file accessible through BACnet's
//! AtomicReadFile and AtomicWriteFile services. It supports both
//! stream access (byte-level) and record access (record-level) modes.
//!
//! ## File Access Methods
//!
//! - **Stream access** (`FileAccessMethod::StreamAccess = 1`):
//!   Data is accessed as a contiguous byte stream. Positions and counts
//!   are in bytes (octets).
//!
//! - **Record access** (`FileAccessMethod::RecordAccess = 0`):
//!   Data is organized as a sequence of variable-length records.
//!   Positions and counts are in record indices.

use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::property::{
    BACnetValue, EventState, PropertyError, PropertyId, PropertyStore, Reliability, StatusFlags,
};
use super::traits::BACnetObject;
use super::types::{ObjectId, ObjectType};

/// File access method per ASHRAE 135.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FileAccessMethod {
    /// Record-based access: data organized as discrete records.
    RecordAccess = 0,
    /// Stream-based access: data as contiguous byte stream.
    StreamAccess = 1,
}

impl FileAccessMethod {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::RecordAccess),
            1 => Some(Self::StreamAccess),
            _ => None,
        }
    }
}

/// BACnet File object.
///
/// Provides simulated file storage accessible via AtomicReadFile/AtomicWriteFile.
/// In stream mode, `file_data` holds raw bytes.
/// In record mode, `records` holds individual record payloads.
pub struct FileObject {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,

    /// File access method (record or stream).
    access_method: FileAccessMethod,
    /// Whether the file is read-only.
    read_only: AtomicBool,
    /// Out-of-service flag.
    out_of_service: AtomicBool,

    // Stream mode storage
    /// Raw file data for stream access.
    file_data: RwLock<Vec<u8>>,

    // Record mode storage
    /// Records for record access.
    records: RwLock<Vec<Vec<u8>>>,

    /// Modification counter (incremented on every write).
    modification_count: AtomicU64,

    /// MIME-like file type descriptor (e.g., "application/octet-stream").
    file_type: RwLock<String>,
}

impl FileObject {
    /// Create a new File object in stream access mode.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        Self::with_access_method(instance, name, FileAccessMethod::StreamAccess)
    }

    /// Create a new File object with a specific access method.
    pub fn with_access_method(
        instance: u32,
        name: impl Into<String>,
        access_method: FileAccessMethod,
    ) -> Self {
        let id = ObjectId::new(ObjectType::File, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );
        properties.set(
            PropertyId::Reliability,
            BACnetValue::Enumerated(Reliability::NoFaultDetected as u32),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            access_method,
            read_only: AtomicBool::new(false),
            out_of_service: AtomicBool::new(false),
            file_data: RwLock::new(Vec::new()),
            records: RwLock::new(Vec::new()),
            modification_count: AtomicU64::new(0),
            file_type: RwLock::new("application/octet-stream".to_string()),
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: set file type.
    pub fn with_file_type(self, ft: impl Into<String>) -> Self {
        *self.file_type.write() = ft.into();
        self
    }

    /// Builder: set read-only flag.
    pub fn with_read_only(self, ro: bool) -> Self {
        self.read_only.store(ro, Ordering::Release);
        self
    }

    /// Builder: set initial stream data.
    pub fn with_data(self, data: Vec<u8>) -> Self {
        *self.file_data.write() = data;
        self
    }

    /// Builder: set initial records.
    pub fn with_records(self, records: Vec<Vec<u8>>) -> Self {
        *self.records.write() = records;
        self
    }

    // ── Stream access operations ────────────────────────────────────

    /// Get the file access method.
    pub fn access_method(&self) -> FileAccessMethod {
        self.access_method
    }

    /// Check if the file is read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only.load(Ordering::Acquire)
    }

    /// Get the total file size in bytes (stream mode).
    pub fn file_size(&self) -> u32 {
        self.file_data.read().len() as u32
    }

    /// Get the record count (record mode).
    pub fn record_count(&self) -> u32 {
        self.records.read().len() as u32
    }

    /// Read a range of bytes from the file (stream access).
    ///
    /// Returns `(data, eof)` where `eof` is true if the end of file was reached.
    pub fn read_stream(&self, start: i32, count: u32) -> (Vec<u8>, bool) {
        let data = self.file_data.read();
        let file_len = data.len();

        if file_len == 0 || count == 0 {
            return (Vec::new(), true);
        }

        // Negative start means from end of file
        let start = if start < 0 {
            let abs_start = (-start) as usize;
            file_len.saturating_sub(abs_start)
        } else {
            start as usize
        };

        if start >= file_len {
            return (Vec::new(), true);
        }

        let end = std::cmp::min(start + count as usize, file_len);
        let eof = end >= file_len;
        (data[start..end].to_vec(), eof)
    }

    /// Write bytes at the given position (stream access).
    ///
    /// Returns the file start position where writing began.
    /// If `start` is -1, data is appended to the end.
    pub fn write_stream(&self, start: i32, new_data: &[u8]) -> Result<i32, PropertyError> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(PropertyError::WriteAccessDenied(PropertyId::FileSize));
        }

        let mut data = self.file_data.write();

        let write_pos = if start < 0 {
            // Append mode
            data.len()
        } else {
            start as usize
        };

        // Extend the file if necessary
        if write_pos + new_data.len() > data.len() {
            data.resize(write_pos + new_data.len(), 0);
        }

        data[write_pos..write_pos + new_data.len()].copy_from_slice(new_data);
        self.modification_count.fetch_add(1, Ordering::Release);

        Ok(write_pos as i32)
    }

    // ── Record access operations ────────────────────────────────────

    /// Read records starting at the given index.
    ///
    /// Returns `(records, eof)`.
    /// Positive `count` reads forward, negative reads backward.
    pub fn read_records(&self, start: i32, count: i32) -> (Vec<Vec<u8>>, bool) {
        let records = self.records.read();
        let total = records.len();

        if total == 0 || count == 0 {
            return (Vec::new(), true);
        }

        let abs_count = count.unsigned_abs() as usize;

        if count > 0 {
            // Forward read
            let start_idx = if start < 0 { 0 } else { start as usize };
            if start_idx >= total {
                return (Vec::new(), true);
            }
            let end = std::cmp::min(start_idx + abs_count, total);
            let eof = end >= total;
            (records[start_idx..end].to_vec(), eof)
        } else {
            // Backward read
            let end_idx = if start < 0 {
                total
            } else {
                std::cmp::min(start as usize + 1, total)
            };
            let start_idx = end_idx.saturating_sub(abs_count);
            let eof = start_idx == 0;
            (records[start_idx..end_idx].to_vec(), eof)
        }
    }

    /// Write records starting at the given index.
    ///
    /// Returns the record index where writing began.
    /// If `start` is -1, records are appended.
    pub fn write_records(
        &self,
        start: i32,
        new_records: Vec<Vec<u8>>,
    ) -> Result<i32, PropertyError> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(PropertyError::WriteAccessDenied(PropertyId::RecordCount));
        }

        let mut records = self.records.write();

        let write_idx = if start < 0 {
            records.len()
        } else {
            start as usize
        };

        // Extend if necessary
        while records.len() < write_idx + new_records.len() {
            records.push(Vec::new());
        }

        for (i, record) in new_records.into_iter().enumerate() {
            records[write_idx + i] = record;
        }

        self.modification_count.fetch_add(1, Ordering::Release);
        Ok(write_idx as i32)
    }

    /// Get all stream data (for testing/inspection).
    pub fn get_data(&self) -> Vec<u8> {
        self.file_data.read().clone()
    }

    /// Set stream data directly (for simulation setup).
    pub fn set_data(&self, data: Vec<u8>) {
        *self.file_data.write() = data;
        self.modification_count.fetch_add(1, Ordering::Release);
    }

    /// Get all records (for testing/inspection).
    pub fn get_records(&self) -> Vec<Vec<u8>> {
        self.records.read().clone()
    }

    /// Set records directly (for simulation setup).
    pub fn set_records(&self, records: Vec<Vec<u8>>) {
        *self.records.write() = records;
        self.modification_count.fetch_add(1, Ordering::Release);
    }

    /// Get modification count.
    pub fn modification_count(&self) -> u64 {
        self.modification_count.load(Ordering::Acquire)
    }
}

impl BACnetObject for FileObject {
    fn object_identifier(&self) -> ObjectId {
        self.id
    }

    fn object_name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        if self.description.is_empty() {
            None
        } else {
            Some(&self.description)
        }
    }

    fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => Ok(BACnetValue::Enumerated(ObjectType::File as u32)),
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::EventState => self
                .properties
                .get(PropertyId::EventState)
                .ok_or(PropertyError::NotFound(PropertyId::EventState)),
            PropertyId::Reliability => self
                .properties
                .get(PropertyId::Reliability)
                .ok_or(PropertyError::NotFound(PropertyId::Reliability)),
            PropertyId::OutOfService => Ok(BACnetValue::Boolean(
                self.out_of_service.load(Ordering::Acquire),
            )),
            PropertyId::FileType => Ok(BACnetValue::CharacterString(self.file_type.read().clone())),
            PropertyId::FileSize => {
                let size = match self.access_method {
                    FileAccessMethod::StreamAccess => self.file_data.read().len() as u32,
                    FileAccessMethod::RecordAccess => {
                        // For record access, file_size is the sum of all record sizes
                        self.records.read().iter().map(|r| r.len() as u32).sum()
                    }
                };
                Ok(BACnetValue::Unsigned(size))
            }
            PropertyId::FileAccessMethod => Ok(BACnetValue::Enumerated(self.access_method as u32)),
            PropertyId::ReadOnly => {
                Ok(BACnetValue::Boolean(self.read_only.load(Ordering::Acquire)))
            }
            PropertyId::RecordCount => Ok(BACnetValue::Unsigned(self.records.read().len() as u32)),
            PropertyId::ModificationDate => {
                // Return modification count as an unsigned value
                // (a real implementation would track actual dates)
                let count = self.modification_count.load(Ordering::Acquire);
                Ok(BACnetValue::Unsigned64(count))
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            // Read-only properties
            PropertyId::ObjectIdentifier
            | PropertyId::ObjectType
            | PropertyId::FileSize
            | PropertyId::FileAccessMethod
            | PropertyId::RecordCount
            | PropertyId::ModificationDate => Err(PropertyError::ReadOnly(property_id)),

            PropertyId::Description => {
                if let Some(s) = value.as_string() {
                    // Description is stored on the struct but we can't mutate it
                    // through &self, so store in the property store as an override
                    self.properties
                        .set(property_id, BACnetValue::CharacterString(s.to_string()));
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::FileType => {
                if let Some(s) = value.as_string() {
                    *self.file_type.write() = s.to_string();
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::ReadOnly => {
                if let Some(v) = value.as_bool() {
                    self.read_only.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        let mut props = vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::Reliability,
            PropertyId::OutOfService,
            PropertyId::FileType,
            PropertyId::FileSize,
            PropertyId::FileAccessMethod,
            PropertyId::ReadOnly,
            PropertyId::ModificationDate,
        ];
        if self.access_method == FileAccessMethod::RecordAccess {
            props.push(PropertyId::RecordCount);
        }
        props
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_object_creation() {
        let file = FileObject::new(1, "TestFile");
        assert_eq!(file.object_identifier(), ObjectId::new(ObjectType::File, 1));
        assert_eq!(file.object_name(), "TestFile");
        assert_eq!(file.access_method(), FileAccessMethod::StreamAccess);
        assert!(!file.is_read_only());
    }

    #[test]
    fn test_file_object_builder() {
        let file = FileObject::with_access_method(2, "RecordFile", FileAccessMethod::RecordAccess)
            .with_description("A test record file")
            .with_file_type("text/plain")
            .with_read_only(true);

        assert_eq!(file.access_method(), FileAccessMethod::RecordAccess);
        assert!(file.is_read_only());
        assert_eq!(file.description(), Some("A test record file"));

        let ft = file.read_property(PropertyId::FileType).unwrap();
        assert_eq!(ft, BACnetValue::CharacterString("text/plain".to_string()));
    }

    #[test]
    fn test_stream_read_write() {
        let file = FileObject::new(1, "StreamFile");

        // Write initial data
        let pos = file.write_stream(0, b"Hello, BACnet!").unwrap();
        assert_eq!(pos, 0);
        assert_eq!(file.file_size(), 14);

        // Read full content
        let (data, eof) = file.read_stream(0, 100);
        assert_eq!(data, b"Hello, BACnet!");
        assert!(eof);

        // Read partial content
        let (data, eof) = file.read_stream(0, 5);
        assert_eq!(data, b"Hello");
        assert!(!eof);

        // Read from middle
        let (data, eof) = file.read_stream(7, 100);
        assert_eq!(data, b"BACnet!");
        assert!(eof);
    }

    #[test]
    fn test_stream_append() {
        let file = FileObject::new(1, "AppendFile");
        file.write_stream(0, b"Hello").unwrap();
        file.write_stream(-1, b" World").unwrap();

        let (data, _) = file.read_stream(0, 100);
        assert_eq!(data, b"Hello World");
    }

    #[test]
    fn test_stream_overwrite() {
        let file = FileObject::new(1, "OverwriteFile");
        file.write_stream(0, b"Hello World").unwrap();
        file.write_stream(6, b"BACnet").unwrap();

        let (data, _) = file.read_stream(0, 100);
        assert_eq!(data, b"Hello BACnet");
    }

    #[test]
    fn test_stream_read_empty() {
        let file = FileObject::new(1, "EmptyFile");
        let (data, eof) = file.read_stream(0, 100);
        assert!(data.is_empty());
        assert!(eof);
    }

    #[test]
    fn test_stream_read_beyond_end() {
        let file = FileObject::new(1, "SmallFile").with_data(b"Hi".to_vec());
        let (data, eof) = file.read_stream(100, 10);
        assert!(data.is_empty());
        assert!(eof);
    }

    #[test]
    fn test_stream_negative_start() {
        let file = FileObject::new(1, "NegFile").with_data(b"Hello World".to_vec());
        // Read last 5 bytes
        let (data, eof) = file.read_stream(-5, 100);
        assert_eq!(data, b"World");
        assert!(eof);
    }

    #[test]
    fn test_record_read_write() {
        let file = FileObject::with_access_method(1, "RecFile", FileAccessMethod::RecordAccess);

        let records = vec![
            b"Record 0".to_vec(),
            b"Record 1".to_vec(),
            b"Record 2".to_vec(),
        ];
        let pos = file.write_records(-1, records).unwrap();
        assert_eq!(pos, 0);
        assert_eq!(file.record_count(), 3);

        // Read all records forward
        let (recs, eof) = file.read_records(0, 10);
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0], b"Record 0");
        assert_eq!(recs[2], b"Record 2");
        assert!(eof);
    }

    #[test]
    fn test_record_read_partial() {
        let file = FileObject::with_access_method(1, "RecFile", FileAccessMethod::RecordAccess)
            .with_records(vec![
                b"R0".to_vec(),
                b"R1".to_vec(),
                b"R2".to_vec(),
                b"R3".to_vec(),
                b"R4".to_vec(),
            ]);

        // Read 2 records starting at index 1
        let (recs, eof) = file.read_records(1, 2);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0], b"R1");
        assert_eq!(recs[1], b"R2");
        assert!(!eof);
    }

    #[test]
    fn test_record_read_backward() {
        let file = FileObject::with_access_method(1, "RecFile", FileAccessMethod::RecordAccess)
            .with_records(vec![
                b"R0".to_vec(),
                b"R1".to_vec(),
                b"R2".to_vec(),
                b"R3".to_vec(),
            ]);

        // Read 2 records backward from index 3
        let (recs, eof) = file.read_records(3, -2);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0], b"R2");
        assert_eq!(recs[1], b"R3");
        assert!(!eof);
    }

    #[test]
    fn test_record_overwrite() {
        let file = FileObject::with_access_method(1, "RecFile", FileAccessMethod::RecordAccess)
            .with_records(vec![b"R0".to_vec(), b"R1".to_vec(), b"R2".to_vec()]);

        file.write_records(1, vec![b"NEW".to_vec()]).unwrap();

        let (recs, _) = file.read_records(0, 10);
        assert_eq!(recs[0], b"R0");
        assert_eq!(recs[1], b"NEW");
        assert_eq!(recs[2], b"R2");
    }

    #[test]
    fn test_read_only_prevents_write() {
        let file = FileObject::new(1, "ReadOnlyFile").with_read_only(true);

        let result = file.write_stream(0, b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_only_records_prevents_write() {
        let file = FileObject::with_access_method(1, "RORecFile", FileAccessMethod::RecordAccess)
            .with_read_only(true);

        let result = file.write_records(0, vec![b"data".to_vec()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_property_reads() {
        let file = FileObject::new(1, "PropFile")
            .with_data(b"Hello".to_vec())
            .with_file_type("text/plain")
            .with_read_only(false);

        assert_eq!(
            file.read_property(PropertyId::ObjectType).unwrap(),
            BACnetValue::Enumerated(ObjectType::File as u32)
        );
        assert_eq!(
            file.read_property(PropertyId::FileSize).unwrap(),
            BACnetValue::Unsigned(5)
        );
        assert_eq!(
            file.read_property(PropertyId::FileType).unwrap(),
            BACnetValue::CharacterString("text/plain".to_string())
        );
        assert_eq!(
            file.read_property(PropertyId::FileAccessMethod).unwrap(),
            BACnetValue::Enumerated(FileAccessMethod::StreamAccess as u32)
        );
        assert_eq!(
            file.read_property(PropertyId::ReadOnly).unwrap(),
            BACnetValue::Boolean(false)
        );
    }

    #[test]
    fn test_property_writes() {
        let file = FileObject::new(1, "WritePropFile");

        // Writable properties
        file.write_property(PropertyId::OutOfService, BACnetValue::Boolean(true))
            .unwrap();
        assert_eq!(
            file.read_property(PropertyId::OutOfService).unwrap(),
            BACnetValue::Boolean(true)
        );

        file.write_property(
            PropertyId::FileType,
            BACnetValue::CharacterString("text/csv".to_string()),
        )
        .unwrap();
        assert_eq!(
            file.read_property(PropertyId::FileType).unwrap(),
            BACnetValue::CharacterString("text/csv".to_string())
        );

        // Read-only properties
        assert!(file
            .write_property(PropertyId::FileSize, BACnetValue::Unsigned(100))
            .is_err());
        assert!(file
            .write_property(PropertyId::FileAccessMethod, BACnetValue::Enumerated(0))
            .is_err());
    }

    #[test]
    fn test_list_properties_stream() {
        let file = FileObject::new(1, "StreamFile");
        let props = file.list_properties();
        assert!(props.contains(&PropertyId::FileSize));
        assert!(props.contains(&PropertyId::FileAccessMethod));
        assert!(props.contains(&PropertyId::ReadOnly));
        // RecordCount should NOT be listed for stream access
        assert!(!props.contains(&PropertyId::RecordCount));
    }

    #[test]
    fn test_list_properties_record() {
        let file = FileObject::with_access_method(1, "RecFile", FileAccessMethod::RecordAccess);
        let props = file.list_properties();
        assert!(props.contains(&PropertyId::RecordCount));
    }

    #[test]
    fn test_modification_count() {
        let file = FileObject::new(1, "ModFile");
        assert_eq!(file.modification_count(), 0);

        file.write_stream(0, b"data").unwrap();
        assert_eq!(file.modification_count(), 1);

        file.write_stream(-1, b"more").unwrap();
        assert_eq!(file.modification_count(), 2);
    }

    #[test]
    fn test_set_data_directly() {
        let file = FileObject::new(1, "DirectFile");
        file.set_data(b"Direct data".to_vec());
        assert_eq!(file.get_data(), b"Direct data");
        assert_eq!(file.modification_count(), 1);
    }

    #[test]
    fn test_status_flags() {
        let file = FileObject::new(1, "FlagsFile");
        let flags = file.status_flags();
        assert!(!flags.in_alarm);
        assert!(!flags.fault);
        assert!(!flags.overridden);
        assert!(!flags.out_of_service);

        file.out_of_service.store(true, Ordering::Release);
        let flags = file.status_flags();
        assert!(flags.out_of_service);
    }

    #[test]
    fn test_file_size_record_mode() {
        let file = FileObject::with_access_method(1, "RecFile", FileAccessMethod::RecordAccess)
            .with_records(vec![
                b"AAAA".to_vec(),   // 4 bytes
                b"BBBBBB".to_vec(), // 6 bytes
            ]);

        // File size in record mode = sum of all record sizes
        assert_eq!(
            file.read_property(PropertyId::FileSize).unwrap(),
            BACnetValue::Unsigned(10)
        );
    }

    #[test]
    fn test_stream_extend_on_write() {
        let file = FileObject::new(1, "ExtendFile");
        // Write at position 10 when file is empty — should fill gap with zeros
        file.write_stream(10, b"data").unwrap();
        let (data, _) = file.read_stream(0, 100);
        assert_eq!(data.len(), 14);
        assert_eq!(&data[..10], &[0u8; 10]);
        assert_eq!(&data[10..], b"data");
    }
}
