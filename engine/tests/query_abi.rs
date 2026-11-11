#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use prdb_engine::query_abi::{
    BorrowedInvocationV1, HostApiV1, HostApiV2, HostApiV3, HostApiV4, InvocationV1, InvocationV2,
    InvocationV3, InvocationV4, ParameterError, QUERY_ABI_V2_VERSION, QUERY_ABI_V3_VERSION,
    QUERY_ABI_V4_VERSION, QUERY_ABI_VERSION, QueryDescriptorV1, QueryDescriptorV2,
    QueryDescriptorV3, QueryDescriptorV4, validate_descriptor_v1, validate_descriptor_v2,
    validate_descriptor_v3, validate_descriptor_v4,
};

#[test]
fn generated_nested_record_codecs_preserve_wire_bytes_and_boundaries() {
    let name =
        prdb_engine::query_abi::encode_executor_resolve_relation_available_response_relation_name(
            "items",
        )
        .unwrap();
    assert_eq!(name, [0, 5, b'i', b't', b'e', b'm', b's']);

    let mut tail = name.clone();
    tail.extend_from_slice(
        &prdb_engine::query_abi::encode_executor_resolve_relation_available_response_column_count(
            2,
        ),
    );
    let (decoded_name, consumed) =
        prdb_engine::query_abi::decode_executor_resolve_relation_available_response_relation_name(
            &tail,
        )
        .unwrap();
    assert_eq!(decoded_name, "items");
    assert_eq!(consumed, name.len());
    assert_eq!(
        prdb_engine::query_abi::decode_executor_resolve_relation_available_response_column_count(
            &tail[consumed..]
        ),
        Some(2)
    );
    assert!(
        prdb_engine::query_abi::decode_executor_resolve_relation_available_response_relation_name(
            &tail[..6]
        )
        .is_none()
    );

    let planner_name =
        prdb_engine::query_abi::encode_planner_describe_relation_available_response_relation_name(
            "items",
        )
        .unwrap();
    assert_eq!(
        prdb_engine::query_abi::decode_planner_describe_relation_available_response_relation_name(
            &planner_name
        ),
        Some(("items", planner_name.len()))
    );
    let plan_name = prdb_engine::query_abi::encode_planner_describe_plan_relation_available_response_relation_name(
        "items",
    )
    .unwrap();
    assert_eq!(
        prdb_engine::query_abi::decode_planner_describe_plan_relation_available_response_relation_name(
            &plan_name
        ),
        Some(("items", plan_name.len()))
    );
    assert_eq!(
        prdb_engine::query_abi::decode_planner_describe_plan_relation_available_response_column_count(
            &[0, 3],
        ),
        Some(3)
    );

    let column =
        prdb_engine::query_abi::encode_visible_column_record([0x42; 16], 0x0102_0304, 1, 3, [0; 4]);
    assert_eq!(
        column,
        [[0x42; 16].as_slice(), &[1, 2, 3, 4, 1, 3, 0, 0, 0, 0],]
            .concat()
            .as_slice()
    );
    assert_eq!(
        prdb_engine::query_abi::decode_visible_column_record(&column),
        Some(([0x42; 16], 0x0102_0304, 1, 3, [0; 4]))
    );

    let value_header = prdb_engine::query_abi::encode_scalar_value_record(3, 4);
    assert_eq!(value_header, [3, 0, 0, 0, 4]);
    assert_eq!(
        prdb_engine::query_abi::decode_scalar_value_record(&value_header),
        Some((3, 4))
    );
    let value_bytes =
        prdb_engine::query_abi::encode_scalar_value_record_value_bytes(&[0xff, 0, 1, 2]).unwrap();
    assert_eq!(
        prdb_engine::query_abi::decode_scalar_value_record_value_bytes(value_bytes, 4),
        Some([0xff, 0, 1, 2].as_slice())
    );
    assert!(
        prdb_engine::query_abi::decode_scalar_value_record_value_bytes(&value_bytes[..3], 4)
            .is_none()
    );

    let patch_ordinal = prdb_engine::query_abi::encode_patch_ordinal_record(0x0102_0304);
    assert_eq!(patch_ordinal, [1, 2, 3, 4]);
    assert_eq!(
        prdb_engine::query_abi::decode_patch_ordinal_record(&patch_ordinal),
        Some(0x0102_0304)
    );
    assert!(prdb_engine::query_abi::decode_patch_ordinal_record(&patch_ordinal[..3]).is_none());

    let row_handle = [8; 16];
    let batch_row = prdb_engine::query_abi::encode_executor_batch_row_record(row_handle, 2);
    assert_eq!(batch_row.len(), 18);
    assert_eq!(&batch_row[..16], &row_handle);
    assert_eq!(&batch_row[16..], &[0, 2]);
    assert_eq!(
        prdb_engine::query_abi::decode_executor_batch_row_record(
            &[batch_row.as_slice(), &[0, 1]].concat()
        ),
        Some((row_handle, 2))
    );
    assert!(prdb_engine::query_abi::decode_executor_batch_row_record(&batch_row[..17]).is_none());
}

#[test]
fn query_abi_v1_layout_matches_the_x86_64_c_contract() {
    assert_eq!(std::mem::size_of::<HostApiV1>(), 8);
    assert_eq!(std::mem::align_of::<HostApiV1>(), 4);
    assert_eq!(std::mem::size_of::<QueryDescriptorV1>(), 16);
    assert_eq!(std::mem::align_of::<QueryDescriptorV1>(), 4);
    assert_eq!(std::mem::size_of::<InvocationV1>(), 24);
    assert_eq!(std::mem::align_of::<InvocationV1>(), 8);
    assert_eq!(std::mem::offset_of!(InvocationV1, parameters), 8);
    assert_eq!(std::mem::offset_of!(InvocationV1, parameters_len), 16);
}

#[test]
fn query_abi_v2_layout_matches_the_x86_64_c_contract() {
    assert_eq!(std::mem::size_of::<HostApiV2>(), 40);
    assert_eq!(std::mem::align_of::<HostApiV2>(), 8);
    assert_eq!(std::mem::offset_of!(HostApiV2, context), 8);
    assert_eq!(std::mem::offset_of!(HostApiV2, executor_request), 16);
    assert_eq!(std::mem::offset_of!(HostApiV2, planner_request), 24);
    assert_eq!(std::mem::offset_of!(HostApiV2, write_result), 32);
    assert_eq!(std::mem::size_of::<QueryDescriptorV2>(), 16);
    assert_eq!(std::mem::align_of::<QueryDescriptorV2>(), 4);
    assert_eq!(std::mem::size_of::<InvocationV2>(), 24);
    assert_eq!(std::mem::align_of::<InvocationV2>(), 8);
    assert_eq!(std::mem::offset_of!(InvocationV2, parameters), 8);
    assert_eq!(std::mem::offset_of!(InvocationV2, parameters_len), 16);
}

#[test]
fn query_abi_v3_layout_and_descriptor_match_the_c_contract() {
    assert_eq!(std::mem::size_of::<HostApiV3>(), 48);
    assert_eq!(std::mem::align_of::<HostApiV3>(), 8);
    assert_eq!(std::mem::offset_of!(HostApiV3, context), 8);
    assert_eq!(std::mem::offset_of!(HostApiV3, executor_request), 16);
    assert_eq!(std::mem::offset_of!(HostApiV3, planner_request), 24);
    assert_eq!(std::mem::offset_of!(HostApiV3, write_result), 32);
    assert_eq!(std::mem::offset_of!(HostApiV3, statement_failure), 40);
    assert_eq!(std::mem::size_of::<QueryDescriptorV3>(), 16);
    assert_eq!(std::mem::size_of::<InvocationV3>(), 24);

    let valid = QueryDescriptorV3 {
        struct_size: std::mem::size_of::<QueryDescriptorV3>() as u32,
        abi_version: QUERY_ABI_V3_VERSION,
        host_api_size: std::mem::size_of::<HostApiV3>() as u32,
        invocation_size: std::mem::size_of::<InvocationV3>() as u32,
    };
    assert!(validate_descriptor_v3(&valid).is_ok());
    let mut invalid = valid;
    invalid.abi_version = QUERY_ABI_V2_VERSION;
    assert!(validate_descriptor_v3(&invalid).is_err());
    invalid = valid;
    invalid.host_api_size -= 1;
    assert!(validate_descriptor_v3(&invalid).is_err());
}

#[test]
fn query_abi_v4_preserves_v3_host_prefix_and_adds_optimizer_callback() {
    assert_eq!(std::mem::size_of::<HostApiV4>(), 56);
    assert_eq!(std::mem::align_of::<HostApiV4>(), 8);
    assert_eq!(std::mem::offset_of!(HostApiV4, v3), 0);
    assert_eq!(std::mem::offset_of!(HostApiV4, optimizer_request), 48);
    assert_eq!(std::mem::size_of::<InvocationV4>(), 24);
    assert_eq!(std::mem::size_of::<QueryDescriptorV4>(), 16);

    let valid = QueryDescriptorV4 {
        struct_size: std::mem::size_of::<QueryDescriptorV4>() as u32,
        abi_version: QUERY_ABI_V4_VERSION,
        host_api_size: std::mem::size_of::<HostApiV4>() as u32,
        invocation_size: std::mem::size_of::<InvocationV4>() as u32,
    };
    assert!(validate_descriptor_v4(&valid).is_ok());
    let mut invalid = valid;
    invalid.abi_version = QUERY_ABI_V3_VERSION;
    assert!(validate_descriptor_v4(&invalid).is_err());
    invalid = valid;
    invalid.host_api_size -= 1;
    assert!(validate_descriptor_v4(&invalid).is_err());
    invalid = valid;
    invalid.invocation_size -= 1;
    assert!(validate_descriptor_v4(&invalid).is_err());
}

#[test]
fn query_invocation_parameters_are_borrowed_bounded_and_null_only_when_empty() {
    let empty_owner = BorrowedInvocationV1::new(&[]).unwrap();
    let empty = empty_owner.as_record();
    assert!(empty.parameters.is_null());
    assert_eq!(empty.parameters_len, 0);

    let parameters = [7, 8, 9];
    let invocation_owner = BorrowedInvocationV1::new(&parameters).unwrap();
    let invocation = invocation_owner.as_record();
    assert_eq!(invocation.parameters, parameters.as_ptr());
    assert_eq!(invocation.parameters_len, parameters.len() as u64);

    let oversized = vec![0; prdb_engine::query_abi::MAX_PARAMETER_BYTES as usize + 1];
    assert_eq!(
        BorrowedInvocationV1::new(&oversized).unwrap_err(),
        ParameterError::TooLarge
    );
}

#[test]
fn query_abi_v1_descriptor_rejects_incompatible_sizes_and_versions() {
    let valid = QueryDescriptorV1 {
        struct_size: std::mem::size_of::<QueryDescriptorV1>() as u32,
        abi_version: QUERY_ABI_VERSION,
        host_api_size: std::mem::size_of::<HostApiV1>() as u32,
        invocation_size: std::mem::size_of::<InvocationV1>() as u32,
    };
    assert!(validate_descriptor_v1(&valid).is_ok());

    let mut incompatible = valid;
    incompatible.abi_version += 1;
    assert!(validate_descriptor_v1(&incompatible).is_err());
    incompatible = valid;
    incompatible.host_api_size -= 1;
    assert!(validate_descriptor_v1(&incompatible).is_err());
    incompatible = valid;
    incompatible.invocation_size -= 1;
    assert!(validate_descriptor_v1(&incompatible).is_err());
    incompatible = valid;
    incompatible.struct_size -= 1;
    assert!(validate_descriptor_v1(&incompatible).is_err());
}

#[test]
fn query_abi_v2_descriptor_rejects_incompatible_sizes_and_versions() {
    let valid = QueryDescriptorV2 {
        struct_size: std::mem::size_of::<QueryDescriptorV2>() as u32,
        abi_version: QUERY_ABI_V2_VERSION,
        host_api_size: std::mem::size_of::<HostApiV2>() as u32,
        invocation_size: std::mem::size_of::<InvocationV2>() as u32,
    };
    assert!(validate_descriptor_v2(&valid).is_ok());

    let mut incompatible = valid;
    incompatible.abi_version = QUERY_ABI_VERSION;
    assert!(validate_descriptor_v2(&incompatible).is_err());
    incompatible = valid;
    incompatible.host_api_size -= 1;
    assert!(validate_descriptor_v2(&incompatible).is_err());
    incompatible = valid;
    incompatible.invocation_size -= 1;
    assert!(validate_descriptor_v2(&incompatible).is_err());
    incompatible = valid;
    incompatible.struct_size -= 1;
    assert!(validate_descriptor_v2(&incompatible).is_err());
}
