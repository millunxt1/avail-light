// TODO: temporary binary to try the JSON-RPC server component alone

use core::convert::TryFrom as _;
use std::collections::HashMap;
use substrate_lite::json_rpc::{methods, websocket_server};

fn main() {
    env_logger::init();
    futures::executor::block_on(async_main())
}

async fn async_main() {
    let chain_spec = substrate_lite::chain_spec::ChainSpec::from_json_bytes(
        &include_bytes!("../polkadot.json")[..],
    )
    .unwrap();

    let mut server = websocket_server::WsServer::new(websocket_server::Config {
        bind_address: "0.0.0.0:9944".parse().unwrap(),
        max_frame_size: 1024 * 1024,
        send_buffer_len: 32,
        capacity: 16,
    })
    .await
    .unwrap();

    let mut next_subscription = 0u64;
    let mut runtime_version_subscriptions = HashMap::new();
    let mut all_heads_subscriptions = HashMap::new();
    let mut new_heads_subscriptions = HashMap::new();
    let mut finalized_heads_subscriptions = HashMap::new();
    let mut storage_subscriptions = HashMap::new();

    struct Subscriptions {
        runtime_version: Vec<String>,
        all_heads: Vec<String>,
        new_heads: Vec<String>,
        finalized_heads: Vec<String>,
        storage: Vec<String>,
    }

    loop {
        let (connection_id, response) = match server.next_event().await {
            websocket_server::Event::ConnectionOpen { .. } => {
                server.accept(Subscriptions {
                    runtime_version: Vec::new(),
                    all_heads: Vec::new(),
                    new_heads: Vec::new(),
                    finalized_heads: Vec::new(),
                    storage: Vec::new(),
                });
                continue;
            }
            websocket_server::Event::ConnectionError {
                connection_id,
                user_data,
            } => {
                for runtime_version in user_data.runtime_version {
                    let _user_data = runtime_version_subscriptions.remove(&runtime_version);
                    debug_assert_eq!(_user_data, Some(connection_id));
                }
                for new_heads in user_data.new_heads {
                    let _user_data = new_heads_subscriptions.remove(&new_heads);
                    debug_assert_eq!(_user_data, Some(connection_id));
                }
                for storage in user_data.storage {
                    let _user_data = storage_subscriptions.remove(&storage);
                    debug_assert_eq!(_user_data, Some(connection_id));
                }
                continue;
            }
            websocket_server::Event::TextFrame {
                connection_id,
                message,
                user_data,
            } => {
                let (request_id, call) = methods::parse_json_call(&message).expect("bad request");
                match call {
                    methods::MethodCall::chain_getBlockHash { height } => {
                        assert_eq!(height, 0);
                        let hash = substrate_lite::calculate_genesis_block_header(
                            chain_spec.genesis_storage(),
                        )
                        .hash();
                        let response =
                            methods::Response::chain_getBlockHash(methods::HashHexString(hash))
                                .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::chain_getHeader { hash } => {
                        // TODO: hash
                        let header = substrate_lite::calculate_genesis_block_header(
                            chain_spec.genesis_storage(),
                        )
                        .scale_encoding()
                        .fold(Vec::new(), |mut a, b| {
                            a.extend_from_slice(b.as_ref());
                            a
                        });
                        let response =
                            methods::Response::chain_getHeader(methods::HexString(header))
                                .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::chain_subscribeAllHeads {} => {
                        let subscription = next_subscription.to_string();
                        next_subscription += 1;

                        let response =
                            methods::Response::chain_subscribeAllHeads(subscription.clone())
                                .to_json_response(request_id);
                        user_data.all_heads.push(subscription.clone());
                        all_heads_subscriptions.insert(subscription, connection_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::chain_subscribeNewHeads {} => {
                        let subscription = next_subscription.to_string();
                        next_subscription += 1;

                        let response =
                            methods::Response::chain_subscribeNewHeads(subscription.clone())
                                .to_json_response(request_id);
                        user_data.new_heads.push(subscription.clone());
                        new_heads_subscriptions.insert(subscription, connection_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::chain_subscribeFinalizedHeads {} => {
                        let subscription = next_subscription.to_string();
                        next_subscription += 1;

                        let response =
                            methods::Response::chain_subscribeFinalizedHeads(subscription.clone())
                                .to_json_response(request_id);
                        user_data.finalized_heads.push(subscription.clone());
                        finalized_heads_subscriptions.insert(subscription, connection_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::rpc_methods {} => {
                        let response = methods::Response::rpc_methods(methods::RpcMethods {
                            version: 1,
                            methods: methods::MethodCall::method_names()
                                .map(|n| n.into())
                                .collect(),
                        })
                        .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::state_queryStorageAt { keys, at } => {
                        // TODO: I have no idea what the API of this function is
                        assert!(at.is_none()); // TODO:

                        let mut out = methods::StorageChangeSet {
                            block: methods::HashHexString(
                                substrate_lite::calculate_genesis_block_header(
                                    chain_spec.genesis_storage(),
                                )
                                .hash(),
                            ),
                            changes: Vec::new(),
                        };

                        for key in keys {
                            let value = chain_spec
                                .genesis_storage()
                                .find(|(k, _)| *k == &key.0[..])
                                .map(|(_, v)| methods::HexString(v.to_owned()));
                            out.changes.push((key, value));
                        }

                        let response = methods::Response::state_queryStorageAt(vec![out])
                            .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::state_getKeysPaged {
                        prefix,
                        count,
                        start_key,
                        hash,
                    } => {
                        assert!(hash.is_none()); // TODO:

                        let mut out = Vec::new();
                        for (k, _) in chain_spec.genesis_storage() {
                            if prefix
                                .as_ref()
                                .map_or(false, |prefix| !k.starts_with(&prefix.0))
                            {
                                continue;
                            }

                            if start_key
                                .as_ref()
                                .map_or(false, |start_key| k < &start_key.0[..])
                            {
                                continue;
                            }

                            out.push(methods::HexString(k.to_owned()));
                        }

                        out.sort_by(|a, b| a.0.cmp(&b.0));
                        out.truncate(usize::try_from(count).unwrap_or(usize::max_value()));

                        let response =
                            methods::Response::state_getKeysPaged(out).to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::state_getMetadata {} => {
                        // TODO: complete hack
                        let metadata =
                            hex::decode(&include_str!("json-rpc-test-metadata-tmp")[..]).unwrap();
                        let response =
                            methods::Response::state_getMetadata(methods::HexString(metadata))
                                .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::state_subscribeRuntimeVersion {} => {
                        let subscription = next_subscription.to_string();
                        next_subscription += 1;

                        let response =
                            methods::Response::state_subscribeRuntimeVersion(subscription.clone())
                                .to_json_response(request_id);
                        user_data.runtime_version.push(subscription.clone());
                        runtime_version_subscriptions.insert(subscription, connection_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::state_subscribeStorage { list } => {
                        // TODO: must send value immediately
                        let subscription = next_subscription.to_string();
                        next_subscription += 1;

                        let response =
                            methods::Response::state_subscribeStorage(subscription.clone())
                                .to_json_response(request_id);
                        user_data.storage.push(subscription.clone());
                        storage_subscriptions.insert(subscription, connection_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::state_getRuntimeVersion {} => {
                        // FIXME: hack
                        let response =
                            methods::Response::state_getRuntimeVersion(methods::RuntimeVersion {
                                spec_name: "polkadot".to_string(),
                                impl_name: "substrate-lite".to_string(),
                                authoring_version: 0,
                                spec_version: 18,
                                impl_version: 0,
                                transaction_version: 4,
                            })
                            .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::system_chain {} => {
                        let response =
                            methods::Response::system_chain(chain_spec.name().to_owned())
                                .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::system_chainType {} => {
                        let response =
                            methods::Response::system_chainType(chain_spec.chain_type().to_owned())
                                .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::system_health {} => {
                        let response = methods::Response::system_health(methods::SystemHealth {
                            is_syncing: true,        // TODO:
                            peers: 1,                // TODO:
                            should_have_peers: true, // TODO:
                        })
                        .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::system_name {} => {
                        let response = methods::Response::system_name("substrate-lite!".to_owned())
                            .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::system_properties {} => {
                        let response = methods::Response::system_properties(
                            serde_json::from_str(chain_spec.properties()).unwrap(),
                        )
                        .to_json_response(request_id);
                        (connection_id, response)
                    }
                    methods::MethodCall::system_version {} => {
                        let response = methods::Response::system_version("1.0.0".to_owned())
                            .to_json_response(request_id);
                        (connection_id, response)
                    }
                    _ => {
                        println!("unimplemented: {:?}", call);
                        continue;
                    }
                }
            }
        };

        server.queue_send(connection_id, response);
    }
}