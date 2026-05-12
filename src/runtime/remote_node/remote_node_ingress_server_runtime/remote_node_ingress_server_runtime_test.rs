mod tests {
    use super::super::{
        discover_authority_socket_paths, extract_target_component,
        is_high_frequency_authority_input, route_transport_envelope, ActiveAuthoritySocketBridge,
        ActiveNodeIngressSession, RemoteNodeIngressServerRuntime,
    };
    use crate::cli::RemoteNetworkConfig;
    use crate::infra::remote_grpc_proto::v1::node_session_envelope::Body;
    use crate::infra::remote_grpc_proto::v1::{
        MirrorBootstrapChunk, MirrorBootstrapComplete, NodeSessionEnvelope, RawPtyInput,
        RouteContext, TargetOutput,
    };
    use crate::infra::remote_grpc_transport::RemoteNodeSessionHandle;
    use crate::runtime::remote_authority_transport_runtime::RemoteAuthorityTransportRuntime;
    use crate::runtime::remote_target_publication_runtime::RemoteTargetPublicationRuntime;
    use std::fs;
    use std::net::Shutdown;
    use std::path::PathBuf;
    use std::process;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn extracts_target_component_for_authority_socket_file() {
        let component = extract_target_component(
            "waitagent-remote-65fb3bc8828a0a50-b1df888881737297-d8273c888e3c986c.sock",
            "peer-a",
        );

        assert_eq!(component.as_deref(), Some("d8273c888e3c986c"));
    }

    #[test]
    fn extracts_target_component_for_scoped_remote_main_slot_socket_file() {
        let component = extract_target_component(
            "waitagent-remote-b27520f164626822-b1df888881737297-d8273c888e3c986c.sock",
            "peer-a",
        );

        assert_eq!(component.as_deref(), Some("d8273c888e3c986c"));
    }

    #[test]
    fn authority_socket_discovery_filters_to_authority() {
        // Clean up any stray files from other tests that use the same authority hash
        for entry in fs::read_dir(std::env::temp_dir()).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains("-b1df888881737297-") || name.contains("-b1df89888173744a-") {
                let _ = fs::remove_file(entry.path());
            }
        }

        let matching_a =
            temp_dir_path("waitagent-remote-b1ec3ae6b7a67e00-b1df888881737297-d8273c888e3c986c");
        let matching_b =
            temp_dir_path("waitagent-remote-44df01ed9b438425-b1df888881737297-d8273f888e3c9d85");
        let matching_scoped =
            temp_dir_path("waitagent-remote-926d755099191094-b1df888881737297-d8273e888e3c9bd2");
        let different_authority =
            temp_dir_path("waitagent-remote-ebb26774420f3fb2-b1df89888173744a-0082fb09e9ea2a17");
        fs::write(&matching_a, b"").expect("matching file should write");
        fs::write(&matching_b, b"").expect("second matching file should write");
        fs::write(&matching_scoped, b"").expect("scoped matching file should write");
        fs::write(&different_authority, b"").expect("other authority file should write");

        let paths = discover_authority_socket_paths("peer-a")
            .expect("authority socket discovery should succeed");
        assert!(paths.contains(&matching_a));
        assert!(paths.contains(&matching_b));
        assert!(paths.contains(&matching_scoped));
        assert!(!paths.contains(&different_authority));

        let _ = fs::remove_file(matching_a);
        let _ = fs::remove_file(matching_b);
        let _ = fs::remove_file(matching_scoped);
        let _ = fs::remove_file(different_authority);
    }

    #[test]
    fn high_frequency_authority_input_skips_bridge_refresh() {
        assert!(is_high_frequency_authority_input(&NodeSessionEnvelope {
            message_id: "raw-input".to_string(),
            sent_at: None,
            session_instance_id: "session-1".to_string(),
            correlation_id: None,
            route: None,
            body: Some(Body::RawPtyInput(RawPtyInput {
                attachment_id: "attach-1".to_string(),
                target_id: "remote-peer:peer-a:shell-1".to_string(),
                console_id: "console-a".to_string(),
                console_host_id: "observer-a".to_string(),
                input_seq: 1,
                session_id: "shell-1".to_string(),
                input_bytes: b"x".to_vec(),
            })),
        }));

        assert!(!is_high_frequency_authority_input(&NodeSessionEnvelope {
            message_id: "output".to_string(),
            sent_at: None,
            session_instance_id: "session-1".to_string(),
            correlation_id: None,
            route: None,
            body: Some(Body::TargetOutput(TargetOutput {
                target_id: "remote-peer:peer-a:shell-1".to_string(),
                output_seq: 1,
                stream: "pty".to_string(),
                session_id: "shell-1".to_string(),
                output_bytes: b"x".to_vec(),
            })),
        }));
    }

    #[test]
    fn ingress_runtime_is_explicitly_scoped_to_one_workspace_socket() {
        let runtime = RemoteNodeIngressServerRuntime::from_build_env_with_network_and_socket(
            RemoteNetworkConfig::default(),
            "wa-socket-a",
        )
        .expect("runtime should build");

        let _ = runtime;
    }

    #[test]
    fn ingress_server_bridges_bootstrap_and_output_into_live_authority_socket() {
        let node_id = "peer-a";
        let socket_path =
            temp_dir_path("waitagent-remote-39cc9903ed327149-b1df888881737297-19fb7615081f4059");
        let socket_path_for_accept = socket_path.clone();
        let listener = std::os::unix::net::UnixListener::bind(&socket_path)
            .expect("authority socket should bind");
        let accept_thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("authority client should connect");
            crate::runtime::remote_node_transport_runtime::read_client_hello(&mut stream)
                .expect("client hello should decode");
            crate::runtime::remote_node_transport_runtime::write_server_hello(
                &mut stream,
                "waitagent-test",
            )
            .expect("server hello should encode");
            let reader = stream.try_clone().expect("stream clone should succeed");
            (reader, stream)
        });

        let transport = Arc::new(
            RemoteAuthorityTransportRuntime::connect(&socket_path, node_id)
                .expect("bridge transport should connect"),
        );
        let active_session_handle = RemoteNodeSessionHandle::new_for_tests(node_id, "session-1").0;
        let mut active = ActiveNodeIngressSession {
            session: active_session_handle,
            bridges: std::collections::HashMap::from([(
                socket_path.clone(),
                ActiveAuthoritySocketBridge {
                    target_component: "19fb7615081f4059".to_string(),
                    transport: transport.clone(),
                },
            )]),
        };
        let publication_runtime = RemoteTargetPublicationRuntime::from_build_env()
            .expect("publication runtime should build");

        route_transport_envelope(
            &publication_runtime,
            node_id,
            mirror_bootstrap_chunk_envelope(),
            Some(&mut active),
        )
        .expect("bootstrap chunk should route");
        route_transport_envelope(
            &publication_runtime,
            node_id,
            mirror_bootstrap_complete_envelope(),
            Some(&mut active),
        )
        .expect("bootstrap complete should route");
        route_transport_envelope(
            &publication_runtime,
            node_id,
            target_output_envelope(),
            Some(&mut active),
        )
        .expect("target output should route");

        let (mut authority_stream, authority_writer) =
            accept_thread.join().expect("accept thread should join");
        let bootstrap_chunk = crate::infra::remote_transport_codec::read_control_plane_envelope(
            &mut authority_stream,
        )
        .expect("bootstrap chunk should arrive");
        match bootstrap_chunk.payload {
            crate::infra::remote_protocol::ControlPlanePayload::MirrorBootstrapChunk(payload) => {
                assert_eq!(payload.session_id, "shell-1");
                assert_eq!(payload.target_id, "remote-peer:peer-a:shell-1");
                assert_eq!(payload.chunk_seq, 1);
                assert_eq!(payload.output_bytes, b"bootstrap");
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        let bootstrap_complete = crate::infra::remote_transport_codec::read_control_plane_envelope(
            &mut authority_stream,
        )
        .expect("bootstrap complete should arrive");
        match bootstrap_complete.payload {
            crate::infra::remote_protocol::ControlPlanePayload::MirrorBootstrapComplete(
                payload,
            ) => {
                assert_eq!(payload.session_id, "shell-1");
                assert_eq!(payload.target_id, "remote-peer:peer-a:shell-1");
                assert_eq!(payload.last_chunk_seq, 1);
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        let target_output = crate::infra::remote_transport_codec::read_control_plane_envelope(
            &mut authority_stream,
        )
        .expect("target output should arrive");
        match target_output.payload {
            crate::infra::remote_protocol::ControlPlanePayload::TargetOutput(payload) => {
                assert_eq!(payload.session_id, "shell-1");
                assert_eq!(payload.target_id, "remote-peer:peer-a:shell-1");
                assert_eq!(payload.output_seq, 7);
                assert_eq!(payload.output_bytes, b"a");
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        let _ = authority_stream.shutdown(Shutdown::Both);
        let _ = authority_writer.shutdown(Shutdown::Both);
        let _ = fs::remove_file(socket_path_for_accept);
        let _ = fs::remove_file(socket_path);
    }

    fn temp_dir_path(file_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{file_name}-{}-{unique}.sock", process::id()))
    }

    fn mirror_bootstrap_chunk_envelope() -> NodeSessionEnvelope {
        NodeSessionEnvelope {
            message_id: "mirror-bootstrap-chunk-1".to_string(),
            sent_at: None,
            session_instance_id: "client-session-1".to_string(),
            correlation_id: None,
            route: Some(RouteContext {
                authority_node_id: Some("peer-a".to_string()),
                target_id: Some("remote-peer:peer-a:shell-1".to_string()),
                attachment_id: None,
                console_id: None,
                console_host_id: None,
                session_id: Some("shell-1".to_string()),
            }),
            body: Some(Body::MirrorBootstrapChunk(MirrorBootstrapChunk {
                target_id: "remote-peer:peer-a:shell-1".to_string(),
                session_id: "shell-1".to_string(),
                chunk_seq: 1,
                stream: "pty".to_string(),
                output_bytes: b"bootstrap".to_vec(),
            })),
        }
    }

    fn mirror_bootstrap_complete_envelope() -> NodeSessionEnvelope {
        NodeSessionEnvelope {
            message_id: "mirror-bootstrap-complete-1".to_string(),
            sent_at: None,
            session_instance_id: "client-session-1".to_string(),
            correlation_id: None,
            route: Some(RouteContext {
                authority_node_id: Some("peer-a".to_string()),
                target_id: Some("remote-peer:peer-a:shell-1".to_string()),
                attachment_id: None,
                console_id: None,
                console_host_id: None,
                session_id: Some("shell-1".to_string()),
            }),
            body: Some(Body::MirrorBootstrapComplete(MirrorBootstrapComplete {
                target_id: "remote-peer:peer-a:shell-1".to_string(),
                session_id: "shell-1".to_string(),
                last_chunk_seq: 1,
                alternate_screen_active: false,
                application_cursor_keys: false,
                cursor_visible: true,
            })),
        }
    }

    fn target_output_envelope() -> NodeSessionEnvelope {
        NodeSessionEnvelope {
            message_id: "target-output-1".to_string(),
            sent_at: None,
            session_instance_id: "client-session-1".to_string(),
            correlation_id: None,
            route: Some(RouteContext {
                authority_node_id: Some("peer-a".to_string()),
                target_id: Some("remote-peer:peer-a:shell-1".to_string()),
                attachment_id: None,
                console_id: None,
                console_host_id: None,
                session_id: Some("shell-1".to_string()),
            }),
            body: Some(Body::TargetOutput(TargetOutput {
                target_id: "remote-peer:peer-a:shell-1".to_string(),
                output_seq: 7,
                stream: "pty".to_string(),
                session_id: "shell-1".to_string(),
                output_bytes: b"a".to_vec(),
            })),
        }
    }
}
