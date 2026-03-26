mod support;

use support::{build_device, run_direct_request, DeviceSpec};

use mabi_modbus::context::BroadcastPolicy;

#[test]
fn standard_function_codes_succeed_on_the_direct_service_contract() {
    let device = build_device(DeviceSpec::dense(1));
    let space = device.address_space();

    space.write_coil(0, true).unwrap();
    space.write_coil(2, true).unwrap();
    space.write_coil(3, true).unwrap();
    space.set_discrete_input(0, true).unwrap();
    space.set_discrete_input(1, true).unwrap();
    space.write_holding_register(0, 0x1234).unwrap();
    space.write_holding_register(1, 0x5678).unwrap();
    space.set_input_register(0, 0x9ABC).unwrap();
    space.set_input_register(1, 0xDEF0).unwrap();
    space.write_holding_register(20, 0x1111).unwrap();
    space.write_holding_register(21, 0x2222).unwrap();

    let unit_context = vec![device.context().clone()];
    let cases = vec![
        (vec![0x01, 0x00, 0x00, 0x00, 0x04], vec![0x01, 0x01, 0x0D]),
        (vec![0x02, 0x00, 0x00, 0x00, 0x04], vec![0x02, 0x01, 0x03]),
        (
            vec![0x03, 0x00, 0x00, 0x00, 0x02],
            vec![0x03, 0x04, 0x12, 0x34, 0x56, 0x78],
        ),
        (
            vec![0x04, 0x00, 0x00, 0x00, 0x02],
            vec![0x04, 0x04, 0x9A, 0xBC, 0xDE, 0xF0],
        ),
        (
            vec![0x05, 0x00, 0x0A, 0xFF, 0x00],
            vec![0x05, 0x00, 0x0A, 0xFF, 0x00],
        ),
        (
            vec![0x06, 0x00, 0x0B, 0x13, 0x57],
            vec![0x06, 0x00, 0x0B, 0x13, 0x57],
        ),
        (
            vec![0x0F, 0x00, 0x10, 0x00, 0x0A, 0x02, 0x4D, 0x01],
            vec![0x0F, 0x00, 0x10, 0x00, 0x0A],
        ),
        (
            vec![0x10, 0x00, 0x12, 0x00, 0x02, 0x04, 0xCA, 0xFE, 0xBA, 0xBE],
            vec![0x10, 0x00, 0x12, 0x00, 0x02],
        ),
        (
            vec![0x16, 0x00, 0x14, 0xFF, 0x00, 0x00, 0x56],
            vec![0x16, 0x00, 0x14, 0xFF, 0x00, 0x00, 0x56],
        ),
        (
            vec![
                0x17, 0x00, 0x14, 0x00, 0x02, 0x00, 0x20, 0x00, 0x02, 0x04, 0xAA, 0xAA, 0xBB, 0xBB,
            ],
            vec![0x17, 0x04, 0x11, 0x56, 0x22, 0x22],
        ),
    ];

    for (request, expected) in cases {
        let response = run_direct_request(
            unit_context.clone(),
            BroadcastPolicy::WriteAll,
            1,
            request.clone(),
        )
        .unwrap_or_else(|error| panic!("request {request:02X?} failed with {error:?}"));
        assert_eq!(
            response, expected,
            "unexpected response for request {request:02X?}"
        );
    }
}
