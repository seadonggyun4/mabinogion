//! BACnet Schedule and Calendar object implementations.
//!
//! Per ASHRAE 135, Clause 12.24 (Schedule) and 12.5 (Calendar).
//!
//! ## Schedule
//! Provides time-based value switching via:
//! - **Weekly_Schedule**: 7-day time/value list (Mon→Sun)
//! - **Exception_Schedule**: date/date-range overrides (higher priority)
//! - **Effective_Period**: date range bounding when the schedule is active
//! - **Schedule_Default**: fallback when no entry matches
//!
//! ## Calendar
//! Represents a named collection of date entries:
//! - Individual dates
//! - Date ranges (start..end)
//! - Weekday/month patterns (using BACnet wildcard encoding)
//! Present_Value is `true` when the current date matches any entry.

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;

use super::property::{
    BACnetDate, BACnetTime, BACnetValue, PropertyError, PropertyId, PropertyStore, StatusFlags,
};
use super::traits::BACnetObject;
use super::types::{ObjectId, ObjectType};

// ── Schedule Types ──────────────────────────────────────────────────────────

/// A single time-value entry in a daily schedule.
///
/// At the specified `time`, the schedule output transitions to `value`.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeValue {
    /// Time of day (hour, minute, second, hundredths).
    pub time: BACnetTime,
    /// Value to output at this time.
    pub value: BACnetValue,
}

impl TimeValue {
    pub fn new(hour: u8, minute: u8, value: BACnetValue) -> Self {
        Self {
            time: BACnetTime {
                hour,
                minute,
                second: 0,
                hundredths: 0,
            },
            value,
        }
    }

    pub fn with_seconds(hour: u8, minute: u8, second: u8, value: BACnetValue) -> Self {
        Self {
            time: BACnetTime {
                hour,
                minute,
                second,
                hundredths: 0,
            },
            value,
        }
    }
}

/// A daily schedule — ordered list of time/value transitions for a single day.
pub type DailySchedule = Vec<TimeValue>;

/// Date range for Effective_Period or exception schedule entries.
#[derive(Debug, Clone, PartialEq)]
pub struct DateRange {
    pub start: BACnetDate,
    pub end: BACnetDate,
}

impl DateRange {
    pub fn new(start: BACnetDate, end: BACnetDate) -> Self {
        Self { start, end }
    }

    /// Check if the given date falls within this range.
    pub fn contains(&self, date: &BACnetDate) -> bool {
        let d = date_to_ordinal(date);
        let s = date_to_ordinal(&self.start);
        let e = date_to_ordinal(&self.end);
        d >= s && d <= e
    }
}

/// Entry type for Calendar date lists and exception schedules.
#[derive(Debug, Clone, PartialEq)]
pub enum CalendarEntry {
    /// A specific date.
    Date(BACnetDate),
    /// A date range (inclusive).
    DateRange(DateRange),
    /// A BACnet date pattern with wildcards (month=255 means "any month", etc.).
    WeekNDay {
        /// Month (1-14, 255 = any).
        month: u8,
        /// Week of month (1-5, 6=last, 255 = any).
        week_of_month: u8,
        /// Day of week (1=Mon..7=Sun, 255 = any).
        day_of_week: u8,
    },
}

impl CalendarEntry {
    /// Check if the given date matches this entry.
    pub fn matches(&self, date: &BACnetDate) -> bool {
        match self {
            CalendarEntry::Date(d) => date_matches_pattern(date, d),
            CalendarEntry::DateRange(range) => range.contains(date),
            CalendarEntry::WeekNDay {
                month,
                week_of_month,
                day_of_week,
            } => {
                // Month check
                if *month != 255 && *month != date.month {
                    // Also handle odd/even months
                    if *month == 13 && date.month % 2 == 0 {
                        return false;
                    }
                    if *month == 14 && date.month % 2 != 0 {
                        return false;
                    }
                    if *month <= 12 && *month != date.month {
                        return false;
                    }
                }

                // Day of week check
                if *day_of_week != 255 && *day_of_week != date.day_of_week {
                    return false;
                }

                // Week of month check
                if *week_of_month != 255 {
                    let week = ((date.day - 1) / 7) + 1;
                    if *week_of_month == 6 {
                        // Last week of the month — approximate: day > 21
                        if date.day <= 21 {
                            return false;
                        }
                    } else if *week_of_month != week {
                        return false;
                    }
                }

                true
            }
        }
    }
}

/// Exception schedule entry — a date/date-range/calendar-reference with an
/// associated daily schedule that overrides the weekly schedule.
#[derive(Debug, Clone)]
pub struct SpecialEvent {
    /// When this exception applies.
    pub period: SpecialEventPeriod,
    /// Time-value transitions for this exception day.
    pub schedule: DailySchedule,
    /// Priority (1-16, lower = higher priority).
    pub priority: u8,
}

/// The period specifier for a SpecialEvent.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecialEventPeriod {
    /// A single calendar entry (date, range, or pattern).
    CalendarEntry(CalendarEntry),
    /// Reference to a Calendar object by object identifier.
    CalendarReference(ObjectId),
}

// ── Helper functions ────────────────────────────────────────────────────────

/// Convert a BACnetDate to an ordinal day number for comparison.
/// Returns a u32 encoding as YYYYMMDD (actual year = year_field + 1900).
fn date_to_ordinal(d: &BACnetDate) -> u32 {
    let year = d.year as u32 + 1900;
    let month = d.month as u32;
    let day = d.day as u32;
    year * 10000 + month * 100 + day
}

/// Check if a concrete date matches a pattern date (supports BACnet wildcards).
fn date_matches_pattern(concrete: &BACnetDate, pattern: &BACnetDate) -> bool {
    // Year
    if pattern.year != 255 && pattern.year != concrete.year {
        return false;
    }
    // Month
    if pattern.month != 255 {
        match pattern.month {
            13 => {
                if concrete.month % 2 == 0 {
                    return false;
                }
            }
            14 => {
                if concrete.month % 2 != 0 {
                    return false;
                }
            }
            m => {
                if m != concrete.month {
                    return false;
                }
            }
        }
    }
    // Day
    if pattern.day != 255 {
        match pattern.day {
            33 => {
                if concrete.day % 2 == 0 {
                    return false;
                }
            }
            34 => {
                if concrete.day % 2 != 0 {
                    return false;
                }
            }
            // 32 = last day of month — trust that concrete.day is correct
            32 => {}
            d => {
                if d != concrete.day {
                    return false;
                }
            }
        }
    }
    // Day of week
    if pattern.day_of_week != 255 && pattern.day_of_week != concrete.day_of_week {
        return false;
    }
    true
}

/// Evaluate a daily schedule for a given time.
///
/// Returns the value from the last entry whose time is <= the given time.
/// If no entry matches (time is before the first entry), returns None.
fn evaluate_daily_schedule(schedule: &[TimeValue], now: &BACnetTime) -> Option<BACnetValue> {
    let now_secs = time_to_seconds(now);
    let mut result = None;

    for entry in schedule {
        let entry_secs = time_to_seconds(&entry.time);
        if entry_secs <= now_secs {
            result = Some(entry.value.clone());
        } else {
            break;
        }
    }

    result
}

/// Convert BACnetTime to seconds since midnight.
fn time_to_seconds(t: &BACnetTime) -> u32 {
    (t.hour as u32) * 3600 + (t.minute as u32) * 60 + (t.second as u32)
}

/// Get the day-of-week index (0=Monday..6=Sunday) for BACnet weekly schedule.
/// BACnet day_of_week: 1=Monday..7=Sunday.
fn bacnet_day_of_week_to_index(day_of_week: u8) -> Option<usize> {
    if day_of_week >= 1 && day_of_week <= 7 {
        Some((day_of_week - 1) as usize)
    } else {
        None
    }
}

// ── Schedule Object ─────────────────────────────────────────────────────────

/// BACnet Schedule object per ASHRAE 135, Clause 12.24.
///
/// Computes a present value based on weekly and exception schedules:
/// 1. Check exception schedule entries (highest priority matching entry wins)
/// 2. If no exception matches, use the weekly schedule for today's day-of-week
/// 3. If neither produces a value, use the schedule default
///
/// Effective_Period bounds when the schedule is active.
pub struct Schedule {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    /// Weekly schedule — 7 daily schedules indexed [0]=Monday..[6]=Sunday.
    weekly_schedule: RwLock<[DailySchedule; 7]>,
    /// Exception schedule entries (sorted by priority, lowest number = highest priority).
    exception_schedule: RwLock<Vec<SpecialEvent>>,
    /// Effective period (date range when schedule is active).
    effective_period: RwLock<Option<DateRange>>,
    /// Schedule default value (fallback when no entry matches).
    schedule_default: RwLock<BACnetValue>,
    /// List of object property references that this schedule writes to.
    list_of_object_property_references: RwLock<Vec<ObjectPropertyReference>>,
    /// Priority for writing (1-16).
    priority_for_writing: RwLock<u8>,
    /// Out of service flag.
    out_of_service: AtomicBool,
    /// Reliability.
    reliability: RwLock<u32>,
    /// Present value (cached, updated by evaluate()).
    present_value: RwLock<BACnetValue>,
}

/// Object property reference for schedule output targets.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPropertyReference {
    pub object_id: ObjectId,
    pub property_id: PropertyId,
    pub array_index: Option<u32>,
}

impl Schedule {
    /// Create a new Schedule object.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        Self {
            id: ObjectId::new(ObjectType::Schedule, instance),
            name: name.into(),
            description: String::new(),
            properties: PropertyStore::new(),
            weekly_schedule: RwLock::new(Default::default()),
            exception_schedule: RwLock::new(Vec::new()),
            effective_period: RwLock::new(None),
            schedule_default: RwLock::new(BACnetValue::Null),
            list_of_object_property_references: RwLock::new(Vec::new()),
            priority_for_writing: RwLock::new(16),
            out_of_service: AtomicBool::new(false),
            reliability: RwLock::new(0),
            present_value: RwLock::new(BACnetValue::Null),
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: set schedule default.
    pub fn with_schedule_default(self, value: BACnetValue) -> Self {
        *self.schedule_default.write() = value;
        self
    }

    /// Builder: set effective period.
    pub fn with_effective_period(self, range: DateRange) -> Self {
        *self.effective_period.write() = Some(range);
        self
    }

    /// Builder: set priority for writing.
    pub fn with_priority(self, priority: u8) -> Self {
        *self.priority_for_writing.write() = priority.min(16).max(1);
        self
    }

    // ── Public API ──

    /// Set the daily schedule for a specific day (0=Monday..6=Sunday).
    pub fn set_daily_schedule(&self, day_index: usize, schedule: DailySchedule) {
        if day_index < 7 {
            self.weekly_schedule.write()[day_index] = schedule;
        }
    }

    /// Get the daily schedule for a specific day.
    pub fn get_daily_schedule(&self, day_index: usize) -> Option<DailySchedule> {
        if day_index < 7 {
            Some(self.weekly_schedule.read()[day_index].clone())
        } else {
            None
        }
    }

    /// Add an exception schedule entry.
    pub fn add_exception(&self, event: SpecialEvent) {
        let mut exceptions = self.exception_schedule.write();
        exceptions.push(event);
        // Keep sorted by priority (ascending = highest priority first)
        exceptions.sort_by_key(|e| e.priority);
    }

    /// Clear all exception schedule entries.
    pub fn clear_exceptions(&self) {
        self.exception_schedule.write().clear();
    }

    /// Get exception count.
    pub fn exception_count(&self) -> usize {
        self.exception_schedule.read().len()
    }

    /// Set the schedule default value.
    pub fn set_schedule_default(&self, value: BACnetValue) {
        *self.schedule_default.write() = value;
    }

    /// Get the schedule default value.
    pub fn schedule_default(&self) -> BACnetValue {
        self.schedule_default.read().clone()
    }

    /// Set the effective period.
    pub fn set_effective_period(&self, range: Option<DateRange>) {
        *self.effective_period.write() = range;
    }

    /// Add an object property reference.
    pub fn add_output_reference(&self, reference: ObjectPropertyReference) {
        self.list_of_object_property_references.write().push(reference);
    }

    /// Get the list of output references.
    pub fn output_references(&self) -> Vec<ObjectPropertyReference> {
        self.list_of_object_property_references.read().clone()
    }

    /// Evaluate the schedule for the given date/time and update the present value.
    ///
    /// This is the core scheduling algorithm:
    /// 1. Check if within the effective period (if set)
    /// 2. Check exception schedule entries (priority order, first match wins)
    /// 3. Fall back to the weekly schedule for the given day of week
    /// 4. If no entry applies, use the schedule default
    pub fn evaluate(&self, date: &BACnetDate, time: &BACnetTime) -> BACnetValue {
        // Check effective period
        if let Some(ref period) = *self.effective_period.read() {
            if !period.contains(date) {
                let default = self.schedule_default.read().clone();
                *self.present_value.write() = default.clone();
                return default;
            }
        }

        // Check exception schedule (priority-sorted, first match wins)
        let exceptions = self.exception_schedule.read();
        for event in exceptions.iter() {
            let matches = match &event.period {
                SpecialEventPeriod::CalendarEntry(entry) => entry.matches(date),
                SpecialEventPeriod::CalendarReference(_) => {
                    // Calendar references would need external resolution;
                    // for the simulator, we skip these (or they can be pre-resolved)
                    false
                }
            };

            if matches {
                if let Some(value) = evaluate_daily_schedule(&event.schedule, time) {
                    *self.present_value.write() = value.clone();
                    return value;
                }
            }
        }
        drop(exceptions);

        // Weekly schedule
        if let Some(day_idx) = bacnet_day_of_week_to_index(date.day_of_week) {
            let weekly = self.weekly_schedule.read();
            if let Some(value) = evaluate_daily_schedule(&weekly[day_idx], time) {
                *self.present_value.write() = value.clone();
                return value;
            }
        }

        // Fallback to schedule default
        let default = self.schedule_default.read().clone();
        *self.present_value.write() = default.clone();
        default
    }

    /// Get the current present value (last evaluated).
    pub fn get_present_value(&self) -> BACnetValue {
        self.present_value.read().clone()
    }

    /// Manually set the present value (e.g., for testing or out-of-service mode).
    pub fn set_present_value(&self, value: BACnetValue) {
        *self.present_value.write() = value;
    }
}

impl BACnetObject for Schedule {
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
            PropertyId::ObjectType => Ok(BACnetValue::Enumerated(ObjectType::Schedule as u32)),
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(self.present_value.read().clone()),
            PropertyId::StatusFlags => {
                let flags = self.status_flags();
                Ok(BACnetValue::BitString(vec![
                    flags.in_alarm,
                    flags.fault,
                    flags.overridden,
                    flags.out_of_service,
                ]))
            }
            PropertyId::EventState => Ok(BACnetValue::Enumerated(0)), // normal
            PropertyId::Reliability => Ok(BACnetValue::Enumerated(*self.reliability.read())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::ScheduleDefault => Ok(self.schedule_default.read().clone()),
            PropertyId::PriorityForWriting => {
                Ok(BACnetValue::Unsigned(*self.priority_for_writing.read() as u32))
            }
            PropertyId::EffectivePeriod => {
                match &*self.effective_period.read() {
                    Some(range) => Ok(BACnetValue::Array(vec![
                        BACnetValue::Date(range.start.clone()),
                        BACnetValue::Date(range.end.clone()),
                    ])),
                    None => Ok(BACnetValue::Null),
                }
            }
            PropertyId::WeeklySchedule => {
                let weekly = self.weekly_schedule.read();
                let days: Vec<BACnetValue> = weekly
                    .iter()
                    .map(|day| {
                        let entries: Vec<BACnetValue> = day
                            .iter()
                            .map(|tv| {
                                BACnetValue::Array(vec![
                                    BACnetValue::Time(tv.time.clone()),
                                    tv.value.clone(),
                                ])
                            })
                            .collect();
                        BACnetValue::List(entries)
                    })
                    .collect();
                Ok(BACnetValue::Array(days))
            }
            PropertyId::ExceptionSchedule => {
                let exceptions = self.exception_schedule.read();
                let entries: Vec<BACnetValue> = exceptions
                    .iter()
                    .map(|_e| BACnetValue::Unsigned(1)) // Simplified encoding
                    .collect();
                Ok(BACnetValue::Array(entries))
            }
            PropertyId::ListOfObjectPropertyReferences => {
                let refs = self.list_of_object_property_references.read();
                let entries: Vec<BACnetValue> = refs
                    .iter()
                    .map(|r| BACnetValue::ObjectIdentifier(r.object_id))
                    .collect();
                Ok(BACnetValue::Array(entries))
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
            PropertyId::ObjectIdentifier | PropertyId::ObjectType => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::ObjectName => {
                // Name is read-only at the BACnet level for Schedule
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::Description => {
                // Description is typically writable
                if let BACnetValue::CharacterString(_) = &value {
                    self.properties.set(property_id, value);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::PresentValue => {
                *self.present_value.write() = value;
                Ok(())
            }
            PropertyId::OutOfService => {
                if let BACnetValue::Boolean(v) = value {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::ScheduleDefault => {
                *self.schedule_default.write() = value;
                Ok(())
            }
            PropertyId::PriorityForWriting => {
                if let BACnetValue::Unsigned(v) = value {
                    let p = (v as u8).min(16).max(1);
                    *self.priority_for_writing.write() = p;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::Reliability => {
                if let BACnetValue::Enumerated(v) = value {
                    *self.reliability.write() = v;
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
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::Reliability,
            PropertyId::OutOfService,
            PropertyId::EffectivePeriod,
            PropertyId::WeeklySchedule,
            PropertyId::ExceptionSchedule,
            PropertyId::ScheduleDefault,
            PropertyId::ListOfObjectPropertyReferences,
            PropertyId::PriorityForWriting,
        ]
    }

    fn has_property(&self, property_id: PropertyId) -> bool {
        self.list_properties().contains(&property_id) || self.properties.get(property_id).is_some()
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: *self.reliability.read() != 0,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn is_out_of_service(&self) -> bool {
        self.out_of_service.load(Ordering::Acquire)
    }

    fn present_value(&self) -> Result<BACnetValue, PropertyError> {
        Ok(self.present_value.read().clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ── Calendar Object ─────────────────────────────────────────────────────────

/// BACnet Calendar object per ASHRAE 135, Clause 12.5.
///
/// A Calendar maintains a list of date entries (dates, ranges, patterns).
/// Its Present_Value is `true` when the current date matches any entry,
/// `false` otherwise. This enables Schedule objects to reference Calendar
/// objects for complex recurring date patterns.
pub struct Calendar {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    /// The date list — entries that define when the calendar is "active".
    date_list: RwLock<Vec<CalendarEntry>>,
    /// Cached present value (true = today matches an entry).
    present_value: RwLock<bool>,
}

impl Calendar {
    /// Create a new Calendar object.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        Self {
            id: ObjectId::new(ObjectType::Calendar, instance),
            name: name.into(),
            description: String::new(),
            properties: PropertyStore::new(),
            date_list: RwLock::new(Vec::new()),
            present_value: RwLock::new(false),
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: add a calendar entry.
    pub fn with_entry(self, entry: CalendarEntry) -> Self {
        self.date_list.write().push(entry);
        self
    }

    // ── Public API ──

    /// Add a date entry to the calendar.
    pub fn add_entry(&self, entry: CalendarEntry) {
        self.date_list.write().push(entry);
    }

    /// Remove all entries.
    pub fn clear_entries(&self) {
        self.date_list.write().clear();
    }

    /// Get entry count.
    pub fn entry_count(&self) -> usize {
        self.date_list.read().len()
    }

    /// Get all entries.
    pub fn entries(&self) -> Vec<CalendarEntry> {
        self.date_list.read().clone()
    }

    /// Evaluate the calendar for the given date and update the present value.
    pub fn evaluate(&self, date: &BACnetDate) -> bool {
        let entries = self.date_list.read();
        let active = entries.iter().any(|entry| entry.matches(date));
        *self.present_value.write() = active;
        active
    }

    /// Get the current present value (last evaluated).
    pub fn get_present_value(&self) -> bool {
        *self.present_value.read()
    }

    /// Manually set the present value.
    pub fn set_present_value(&self, value: bool) {
        *self.present_value.write() = value;
    }
}

impl BACnetObject for Calendar {
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
            PropertyId::ObjectType => Ok(BACnetValue::Enumerated(ObjectType::Calendar as u32)),
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(BACnetValue::Boolean(*self.present_value.read())),
            PropertyId::StatusFlags => {
                Ok(BACnetValue::BitString(vec![false, false, false, false]))
            }
            PropertyId::DateList => {
                let entries = self.date_list.read();
                let encoded: Vec<BACnetValue> = entries
                    .iter()
                    .map(|entry| match entry {
                        CalendarEntry::Date(d) => BACnetValue::Date(d.clone()),
                        CalendarEntry::DateRange(range) => BACnetValue::Array(vec![
                            BACnetValue::Date(range.start.clone()),
                            BACnetValue::Date(range.end.clone()),
                        ]),
                        CalendarEntry::WeekNDay {
                            month,
                            week_of_month,
                            day_of_week,
                        } => BACnetValue::OctetString(vec![*month, *week_of_month, *day_of_week]),
                    })
                    .collect();
                Ok(BACnetValue::List(encoded))
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
            PropertyId::ObjectIdentifier | PropertyId::ObjectType => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::ObjectName => Err(PropertyError::ReadOnly(property_id)),
            PropertyId::Description => {
                if let BACnetValue::CharacterString(_) = &value {
                    self.properties.set(property_id, value);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::PresentValue => {
                if let BACnetValue::Boolean(v) = value {
                    *self.present_value.write() = v;
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
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::DateList,
        ]
    }

    fn has_property(&self, property_id: PropertyId) -> bool {
        self.list_properties().contains(&property_id) || self.properties.get(property_id).is_some()
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: false,
        }
    }

    fn is_out_of_service(&self) -> bool {
        false
    }

    fn present_value(&self) -> Result<BACnetValue, PropertyError> {
        Ok(BACnetValue::Boolean(*self.present_value.read()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_date(year: u16, month: u8, day: u8, dow: u8) -> BACnetDate {
        BACnetDate {
            year: (year - 1900) as u8,
            month,
            day,
            day_of_week: dow,
        }
    }

    fn make_time(hour: u8, minute: u8) -> BACnetTime {
        BACnetTime {
            hour,
            minute,
            second: 0,
            hundredths: 0,
        }
    }

    // ── Calendar Entry Matching ──

    #[test]
    fn test_calendar_entry_date_match() {
        let entry = CalendarEntry::Date(make_date(2025, 12, 25, 4)); // Thursday
        let date = make_date(2025, 12, 25, 4);
        assert!(entry.matches(&date));

        let other = make_date(2025, 12, 26, 5);
        assert!(!entry.matches(&other));
    }

    #[test]
    fn test_calendar_entry_date_range() {
        let entry = CalendarEntry::DateRange(DateRange::new(
            make_date(2025, 1, 1, 3),
            make_date(2025, 1, 31, 5),
        ));

        assert!(entry.matches(&make_date(2025, 1, 15, 3)));
        assert!(entry.matches(&make_date(2025, 1, 1, 3)));
        assert!(entry.matches(&make_date(2025, 1, 31, 5)));
        assert!(!entry.matches(&make_date(2025, 2, 1, 6)));
        assert!(!entry.matches(&make_date(2024, 12, 31, 2)));
    }

    #[test]
    fn test_calendar_entry_weeknday() {
        // Every Monday in any month
        let entry = CalendarEntry::WeekNDay {
            month: 255,
            week_of_month: 255,
            day_of_week: 1, // Monday
        };

        let monday = make_date(2025, 3, 10, 1);
        let tuesday = make_date(2025, 3, 11, 2);
        assert!(entry.matches(&monday));
        assert!(!entry.matches(&tuesday));
    }

    #[test]
    fn test_calendar_entry_weeknday_specific_month() {
        // Every Friday in December
        let entry = CalendarEntry::WeekNDay {
            month: 12,
            week_of_month: 255,
            day_of_week: 5, // Friday
        };

        let dec_friday = make_date(2025, 12, 5, 5);
        let jan_friday = make_date(2025, 1, 3, 5);
        assert!(entry.matches(&dec_friday));
        assert!(!entry.matches(&jan_friday));
    }

    #[test]
    fn test_date_pattern_wildcard_year() {
        let pattern = BACnetDate {
            year: 255,
            month: 12,
            day: 25,
            day_of_week: 255,
        };
        // Christmas any year
        assert!(date_matches_pattern(&make_date(2025, 12, 25, 4), &pattern));
        assert!(date_matches_pattern(&make_date(2030, 12, 25, 3), &pattern));
        assert!(!date_matches_pattern(&make_date(2025, 12, 26, 5), &pattern));
    }

    // ── Daily Schedule Evaluation ──

    #[test]
    fn test_evaluate_daily_schedule() {
        let schedule = vec![
            TimeValue::new(8, 0, BACnetValue::Real(72.0)),
            TimeValue::new(12, 0, BACnetValue::Real(70.0)),
            TimeValue::new(18, 0, BACnetValue::Real(65.0)),
        ];

        // Before first entry
        assert_eq!(evaluate_daily_schedule(&schedule, &make_time(7, 30)), None);

        // At first entry
        assert_eq!(
            evaluate_daily_schedule(&schedule, &make_time(8, 0)),
            Some(BACnetValue::Real(72.0))
        );

        // Between entries
        assert_eq!(
            evaluate_daily_schedule(&schedule, &make_time(10, 0)),
            Some(BACnetValue::Real(72.0))
        );

        // At second entry
        assert_eq!(
            evaluate_daily_schedule(&schedule, &make_time(12, 0)),
            Some(BACnetValue::Real(70.0))
        );

        // After last entry
        assert_eq!(
            evaluate_daily_schedule(&schedule, &make_time(23, 0)),
            Some(BACnetValue::Real(65.0))
        );
    }

    #[test]
    fn test_evaluate_empty_schedule() {
        let schedule: Vec<TimeValue> = vec![];
        assert_eq!(evaluate_daily_schedule(&schedule, &make_time(12, 0)), None);
    }

    // ── Schedule Object ──

    #[test]
    fn test_schedule_creation() {
        let schedule = Schedule::new(1, "Test Schedule");
        assert_eq!(schedule.object_identifier(), ObjectId::new(ObjectType::Schedule, 1));
        assert_eq!(schedule.object_name(), "Test Schedule");
        assert_eq!(schedule.get_present_value(), BACnetValue::Null);
    }

    #[test]
    fn test_schedule_weekly_evaluation() {
        let schedule = Schedule::new(1, "HVAC Schedule")
            .with_schedule_default(BACnetValue::Real(60.0));

        // Set Monday schedule
        schedule.set_daily_schedule(0, vec![
            TimeValue::new(8, 0, BACnetValue::Real(72.0)),
            TimeValue::new(18, 0, BACnetValue::Real(65.0)),
        ]);

        // Evaluate Monday 10:00 → should be 72.0
        let monday = make_date(2025, 3, 10, 1); // Monday
        let result = schedule.evaluate(&monday, &make_time(10, 0));
        assert_eq!(result, BACnetValue::Real(72.0));

        // Evaluate Monday 20:00 → should be 65.0
        let result = schedule.evaluate(&monday, &make_time(20, 0));
        assert_eq!(result, BACnetValue::Real(65.0));

        // Evaluate Tuesday (no schedule set) → should be schedule default
        let tuesday = make_date(2025, 3, 11, 2);
        let result = schedule.evaluate(&tuesday, &make_time(10, 0));
        assert_eq!(result, BACnetValue::Real(60.0));
    }

    #[test]
    fn test_schedule_exception_overrides_weekly() {
        let schedule = Schedule::new(1, "Holiday Schedule")
            .with_schedule_default(BACnetValue::Real(60.0));

        // Normal Monday
        schedule.set_daily_schedule(0, vec![
            TimeValue::new(8, 0, BACnetValue::Real(72.0)),
        ]);

        // Exception for holiday: run at 55.0 all day
        schedule.add_exception(SpecialEvent {
            period: SpecialEventPeriod::CalendarEntry(CalendarEntry::Date(
                make_date(2025, 12, 25, 4),
            )),
            schedule: vec![TimeValue::new(0, 0, BACnetValue::Real(55.0))],
            priority: 1,
        });

        // Holiday → exception wins
        let christmas = make_date(2025, 12, 25, 4);
        let result = schedule.evaluate(&christmas, &make_time(10, 0));
        assert_eq!(result, BACnetValue::Real(55.0));

        // Normal Monday → weekly schedule
        let monday = make_date(2025, 3, 10, 1);
        let result = schedule.evaluate(&monday, &make_time(10, 0));
        assert_eq!(result, BACnetValue::Real(72.0));
    }

    #[test]
    fn test_schedule_effective_period() {
        let schedule = Schedule::new(1, "Seasonal Schedule")
            .with_schedule_default(BACnetValue::Real(60.0))
            .with_effective_period(DateRange::new(
                make_date(2025, 6, 1, 255),
                make_date(2025, 8, 31, 255),
            ));

        // Set Monday schedule
        schedule.set_daily_schedule(0, vec![
            TimeValue::new(8, 0, BACnetValue::Real(72.0)),
        ]);

        // Within effective period → normal evaluation
        let july_monday = make_date(2025, 7, 7, 1);
        let result = schedule.evaluate(&july_monday, &make_time(10, 0));
        assert_eq!(result, BACnetValue::Real(72.0));

        // Outside effective period → schedule default
        let jan_monday = make_date(2025, 1, 6, 1);
        let result = schedule.evaluate(&jan_monday, &make_time(10, 0));
        assert_eq!(result, BACnetValue::Real(60.0));
    }

    #[test]
    fn test_schedule_exception_priority() {
        let schedule = Schedule::new(1, "Priority Test")
            .with_schedule_default(BACnetValue::Real(60.0));

        // Low priority exception (runs at 65.0)
        schedule.add_exception(SpecialEvent {
            period: SpecialEventPeriod::CalendarEntry(CalendarEntry::Date(
                make_date(2025, 12, 25, 4),
            )),
            schedule: vec![TimeValue::new(0, 0, BACnetValue::Real(65.0))],
            priority: 10,
        });

        // High priority exception (runs at 55.0) — this should win
        schedule.add_exception(SpecialEvent {
            period: SpecialEventPeriod::CalendarEntry(CalendarEntry::Date(
                make_date(2025, 12, 25, 4),
            )),
            schedule: vec![TimeValue::new(0, 0, BACnetValue::Real(55.0))],
            priority: 1,
        });

        let christmas = make_date(2025, 12, 25, 4);
        let result = schedule.evaluate(&christmas, &make_time(10, 0));
        assert_eq!(result, BACnetValue::Real(55.0));
    }

    #[test]
    fn test_schedule_properties() {
        let schedule = Schedule::new(1, "Prop Test")
            .with_description("Test schedule")
            .with_schedule_default(BACnetValue::Real(68.0))
            .with_priority(8);

        let id = schedule.read_property(PropertyId::ObjectIdentifier).unwrap();
        assert_eq!(id, BACnetValue::ObjectIdentifier(ObjectId::new(ObjectType::Schedule, 1)));

        let name = schedule.read_property(PropertyId::ObjectName).unwrap();
        assert_eq!(name, BACnetValue::CharacterString("Prop Test".into()));

        let obj_type = schedule.read_property(PropertyId::ObjectType).unwrap();
        assert_eq!(obj_type, BACnetValue::Enumerated(17));

        let default = schedule.read_property(PropertyId::ScheduleDefault).unwrap();
        assert_eq!(default, BACnetValue::Real(68.0));

        let priority = schedule.read_property(PropertyId::PriorityForWriting).unwrap();
        assert_eq!(priority, BACnetValue::Unsigned(8));
    }

    #[test]
    fn test_schedule_write_properties() {
        let schedule = Schedule::new(1, "Write Test");

        // Write present value
        schedule
            .write_property(PropertyId::PresentValue, BACnetValue::Real(75.0))
            .unwrap();
        assert_eq!(schedule.get_present_value(), BACnetValue::Real(75.0));

        // Write schedule default
        schedule
            .write_property(PropertyId::ScheduleDefault, BACnetValue::Real(65.0))
            .unwrap();
        assert_eq!(schedule.schedule_default(), BACnetValue::Real(65.0));

        // Write out of service
        schedule
            .write_property(PropertyId::OutOfService, BACnetValue::Boolean(true))
            .unwrap();
        assert!(schedule.is_out_of_service());

        // Read-only rejection
        let result = schedule.write_property(
            PropertyId::ObjectIdentifier,
            BACnetValue::Unsigned(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_schedule_list_properties() {
        let schedule = Schedule::new(1, "List Test");
        let props = schedule.list_properties();
        assert!(props.contains(&PropertyId::WeeklySchedule));
        assert!(props.contains(&PropertyId::ExceptionSchedule));
        assert!(props.contains(&PropertyId::ScheduleDefault));
        assert!(props.contains(&PropertyId::EffectivePeriod));
        assert!(props.contains(&PropertyId::PriorityForWriting));
    }

    #[test]
    fn test_schedule_output_references() {
        let schedule = Schedule::new(1, "Ref Test");
        assert_eq!(schedule.output_references().len(), 0);

        schedule.add_output_reference(ObjectPropertyReference {
            object_id: ObjectId::new(ObjectType::AnalogOutput, 1),
            property_id: PropertyId::PresentValue,
            array_index: None,
        });

        assert_eq!(schedule.output_references().len(), 1);
    }

    // ── Calendar Object ──

    #[test]
    fn test_calendar_creation() {
        let calendar = Calendar::new(1, "Holidays");
        assert_eq!(calendar.object_identifier(), ObjectId::new(ObjectType::Calendar, 1));
        assert_eq!(calendar.object_name(), "Holidays");
        assert!(!calendar.get_present_value());
    }

    #[test]
    fn test_calendar_date_match() {
        let calendar = Calendar::new(1, "Test Calendar")
            .with_entry(CalendarEntry::Date(make_date(2025, 12, 25, 4)));

        assert!(calendar.evaluate(&make_date(2025, 12, 25, 4)));
        assert!(!calendar.evaluate(&make_date(2025, 12, 26, 5)));
    }

    #[test]
    fn test_calendar_date_range_match() {
        let calendar = Calendar::new(1, "Summer Holidays")
            .with_entry(CalendarEntry::DateRange(DateRange::new(
                make_date(2025, 7, 1, 255),
                make_date(2025, 7, 31, 255),
            )));

        assert!(calendar.evaluate(&make_date(2025, 7, 15, 2)));
        assert!(!calendar.evaluate(&make_date(2025, 8, 1, 5)));
    }

    #[test]
    fn test_calendar_weeknday_match() {
        let calendar = Calendar::new(1, "Weekends")
            .with_entry(CalendarEntry::WeekNDay {
                month: 255,
                week_of_month: 255,
                day_of_week: 6, // Saturday
            })
            .with_entry(CalendarEntry::WeekNDay {
                month: 255,
                week_of_month: 255,
                day_of_week: 7, // Sunday
            });

        let saturday = make_date(2025, 3, 8, 6);
        let sunday = make_date(2025, 3, 9, 7);
        let monday = make_date(2025, 3, 10, 1);

        assert!(calendar.evaluate(&saturday));
        assert!(calendar.evaluate(&sunday));
        assert!(!calendar.evaluate(&monday));
    }

    #[test]
    fn test_calendar_properties() {
        let calendar = Calendar::new(1, "Prop Calendar")
            .with_description("Test")
            .with_entry(CalendarEntry::Date(make_date(2025, 1, 1, 3)));

        let id = calendar.read_property(PropertyId::ObjectIdentifier).unwrap();
        assert_eq!(id, BACnetValue::ObjectIdentifier(ObjectId::new(ObjectType::Calendar, 1)));

        let name = calendar.read_property(PropertyId::ObjectName).unwrap();
        assert_eq!(name, BACnetValue::CharacterString("Prop Calendar".into()));

        let obj_type = calendar.read_property(PropertyId::ObjectType).unwrap();
        assert_eq!(obj_type, BACnetValue::Enumerated(6)); // Calendar = 6

        let date_list = calendar.read_property(PropertyId::DateList).unwrap();
        if let BACnetValue::List(entries) = date_list {
            assert_eq!(entries.len(), 1);
        } else {
            panic!("Expected List for DateList");
        }

        let pv = calendar.read_property(PropertyId::PresentValue).unwrap();
        assert_eq!(pv, BACnetValue::Boolean(false));
    }

    #[test]
    fn test_calendar_write_present_value() {
        let calendar = Calendar::new(1, "Write Test");

        calendar
            .write_property(PropertyId::PresentValue, BACnetValue::Boolean(true))
            .unwrap();
        assert!(calendar.get_present_value());

        // Read-only rejection
        let result = calendar.write_property(
            PropertyId::ObjectIdentifier,
            BACnetValue::Unsigned(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_calendar_list_properties() {
        let calendar = Calendar::new(1, "List Test");
        let props = calendar.list_properties();
        assert!(props.contains(&PropertyId::DateList));
        assert!(props.contains(&PropertyId::PresentValue));
        assert!(props.contains(&PropertyId::StatusFlags));
    }

    #[test]
    fn test_calendar_multiple_entries() {
        let calendar = Calendar::new(1, "Multi")
            .with_entry(CalendarEntry::Date(make_date(2025, 1, 1, 3)))
            .with_entry(CalendarEntry::Date(make_date(2025, 7, 4, 5)))
            .with_entry(CalendarEntry::Date(make_date(2025, 12, 25, 4)));

        assert_eq!(calendar.entry_count(), 3);
        assert!(calendar.evaluate(&make_date(2025, 7, 4, 5)));
        assert!(!calendar.evaluate(&make_date(2025, 7, 5, 6)));
    }

    #[test]
    fn test_calendar_clear_entries() {
        let calendar = Calendar::new(1, "Clear Test")
            .with_entry(CalendarEntry::Date(make_date(2025, 1, 1, 3)));

        assert_eq!(calendar.entry_count(), 1);
        calendar.clear_entries();
        assert_eq!(calendar.entry_count(), 0);
    }

    // ── Date Range ──

    #[test]
    fn test_date_range_contains() {
        let range = DateRange::new(
            make_date(2025, 6, 1, 255),
            make_date(2025, 8, 31, 255),
        );

        assert!(range.contains(&make_date(2025, 7, 15, 2)));
        assert!(range.contains(&make_date(2025, 6, 1, 255)));
        assert!(range.contains(&make_date(2025, 8, 31, 255)));
        assert!(!range.contains(&make_date(2025, 5, 31, 6)));
        assert!(!range.contains(&make_date(2025, 9, 1, 1)));
    }

    // ── Time/Value ──

    #[test]
    fn test_time_to_seconds() {
        assert_eq!(time_to_seconds(&make_time(0, 0)), 0);
        assert_eq!(time_to_seconds(&make_time(1, 0)), 3600);
        assert_eq!(time_to_seconds(&make_time(12, 30)), 45000);
        assert_eq!(time_to_seconds(&make_time(23, 59)), 86340);
    }

    #[test]
    fn test_bacnet_day_of_week_to_index() {
        assert_eq!(bacnet_day_of_week_to_index(1), Some(0)); // Monday
        assert_eq!(bacnet_day_of_week_to_index(7), Some(6)); // Sunday
        assert_eq!(bacnet_day_of_week_to_index(0), None);
        assert_eq!(bacnet_day_of_week_to_index(8), None);
    }
}
