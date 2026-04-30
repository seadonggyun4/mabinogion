use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use mabi_bacnet::object::property::{BACnetDate, BACnetTime};
use mabi_bacnet::object::BACnetObject;
use mabi_bacnet::prelude::{
    AnalogInput, AnalogOutput, BACnetValue, Calendar, CalendarEntry, DateRange, FileObject,
    LogDatum, ObjectId, ObjectRegistry, Schedule, ServerConfig, SpecialEvent, SpecialEventPeriod,
    TimeValue, TrendLog,
};

pub struct PropertyFixture {
    pub registry: ObjectRegistry,
    pub analog_output: Arc<AnalogOutput>,
}

pub struct CovFixture {
    pub registry: ObjectRegistry,
    pub analog_input: Arc<AnalogInput>,
    pub object_id: ObjectId,
}

pub struct FileAndTrendFixture {
    pub registry: ObjectRegistry,
    pub file: Arc<FileObject>,
    pub trend_log: Arc<TrendLog>,
    pub file_object_id: ObjectId,
    pub trend_log_id: ObjectId,
}

pub struct ScheduleCalendarFixture {
    pub registry: ObjectRegistry,
    pub schedule: Arc<Schedule>,
    pub calendar: Arc<Calendar>,
    pub schedule_id: ObjectId,
    pub calendar_id: ObjectId,
}

pub struct SegmentationFixture {
    pub registry: ObjectRegistry,
    pub file_object_id: ObjectId,
}

pub fn loopback_server_config(device_instance: u32) -> ServerConfig {
    ServerConfig {
        bind_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        broadcast_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 47_808)),
        device_instance,
        device_name: format!("BACnet Test Device {device_instance}"),
        vendor_id: 2604,
        model_name: "Mabinogion BACnet Test Harness".into(),
        max_apdu_length: 512,
        max_cov_subscriptions: 32,
        cov_check_interval: Duration::from_millis(25),
        shutdown_timeout: Duration::from_secs(2),
    }
}

pub fn property_fixture() -> PropertyFixture {
    let registry = ObjectRegistry::new();
    let analog_output = Arc::new(
        AnalogOutput::new(10, "AO_10")
            .with_description("deterministic integration analog output")
            .with_relinquish_default(12.5),
    );
    registry.register(analog_output.clone());
    PropertyFixture {
        registry,
        analog_output,
    }
}

pub fn cov_fixture() -> CovFixture {
    let registry = ObjectRegistry::new();
    let analog_input = Arc::new(
        AnalogInput::new(20, "AI_20")
            .with_description("deterministic integration cov sensor")
            .with_cov_increment(0.1),
    );
    analog_input.set_value(19.5);
    let object_id = analog_input.object_identifier();
    registry.register(analog_input.clone());
    CovFixture {
        registry,
        analog_input,
        object_id,
    }
}

pub fn file_and_trend_fixture() -> FileAndTrendFixture {
    let registry = ObjectRegistry::new();
    let file = Arc::new(FileObject::new(30, "File_30").with_description("integration file"));
    let trend_log = Arc::new(TrendLog::new(31, "Trend_31", 16).with_enabled(true));
    trend_log.add_record(LogDatum::RealValue(72.0));
    trend_log.add_record(LogDatum::RealValue(73.5));
    let file_object_id = file.object_identifier();
    let trend_log_id = trend_log.object_identifier();
    registry.register(file.clone());
    registry.register(trend_log.clone());
    FileAndTrendFixture {
        registry,
        file,
        trend_log,
        file_object_id,
        trend_log_id,
    }
}

pub fn schedule_calendar_fixture() -> ScheduleCalendarFixture {
    let registry = ObjectRegistry::new();
    let schedule = Arc::new(
        Schedule::new(40, "Schedule_40")
            .with_schedule_default(BACnetValue::Real(60.0))
            .with_effective_period(DateRange::new(
                make_date(2026, 1, 1, 4),
                make_date(2026, 12, 31, 4),
            )),
    );
    schedule.set_daily_schedule(
        0,
        vec![
            TimeValue::new(8, 0, BACnetValue::Real(72.0)),
            TimeValue::new(18, 0, BACnetValue::Real(65.0)),
        ],
    );
    schedule.add_exception(SpecialEvent {
        period: SpecialEventPeriod::CalendarEntry(CalendarEntry::Date(make_date(2026, 12, 25, 5))),
        schedule: vec![TimeValue::new(0, 0, BACnetValue::Real(55.0))],
        priority: 1,
    });
    let calendar = Arc::new(
        Calendar::new(41, "Calendar_41")
            .with_entry(CalendarEntry::Date(make_date(2026, 12, 25, 5))),
    );
    let schedule_id = schedule.object_identifier();
    let calendar_id = calendar.object_identifier();
    registry.register(schedule.clone());
    registry.register(calendar.clone());
    ScheduleCalendarFixture {
        registry,
        schedule,
        calendar,
        schedule_id,
        calendar_id,
    }
}

pub fn segmentation_fixture() -> SegmentationFixture {
    let registry = ObjectRegistry::new();
    let payload: Vec<u8> = (0..=255).map(|value| value as u8).collect();
    let file = Arc::new(
        FileObject::new(50, "File_50")
            .with_description("segmentation file")
            .with_data(payload),
    );
    let file_object_id = file.object_identifier();
    registry.register(file);
    SegmentationFixture {
        registry,
        file_object_id,
    }
}

pub fn make_date(year: u16, month: u8, day: u8, day_of_week: u8) -> BACnetDate {
    BACnetDate {
        year: (year - 1900) as u8,
        month,
        day,
        day_of_week,
    }
}

pub fn make_time(hour: u8, minute: u8, second: u8) -> BACnetTime {
    BACnetTime {
        hour,
        minute,
        second,
        hundredths: 0,
    }
}
