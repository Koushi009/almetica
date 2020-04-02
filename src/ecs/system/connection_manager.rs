use std::collections::HashMap;
use std::str::from_utf8;
use std::sync::Arc;

use crate::ecs::component::Connection;
use crate::ecs::event::Event;
use crate::ecs::event::EventKind;
use crate::ecs::resource::ConnectionMapping;
use crate::ecs::tag;
use crate::protocol::packet::*;
use crate::*;

use legion::prelude::*;
use legion::systems::schedule::Schedulable;
use legion::systems::{SubWorld, SystemBuilder};
use tokio::sync::mpsc::Sender;
use tracing::{debug, error, info_span, trace};

/// Connection handler handles the connection components.
pub fn init(world_id: usize) -> Box<dyn Schedulable> {
    SystemBuilder::new("ConnectionManager")
        .write_resource::<ConnectionMapping>()
        .with_query(<Read<Arc<Event>>>::query().filter(tag_value(&tag::EventKind(EventKind::Request))))
        .write_component::<Arc<Event>>()
        .write_component::<Connection>()
        .build(move |mut command_buffer, mut world, connection_mapping, queries| {
            let span = info_span!("world", world_id);
            let _enter = span.enter();

            for event in queries.iter_mut(&mut *world) {
                match &**event {
                    Event::RequestRegisterConnection { response_channel, .. } => {
                        handle_connection_registration(
                            &mut connection_mapping.map,
                            response_channel,
                            &mut command_buffer,
                        );
                    }
                    Event::RequestCheckVersion { connection, packet } => {
                        if let Err(e) =
                            handle_request_check_version(*connection, &packet, &mut world, &mut command_buffer)
                        {
                            debug!("Can't handle RequestCheckVersion event: {:?}", e);
                        }
                    }
                    Event::RequestLoginArbiter { connection, packet } => {
                        if let Err(e) =
                            handle_request_login_arbiter(*connection, &packet, &mut world, &mut command_buffer)
                        {
                            debug!("Can't handle RequestLoginArbiter event: {:?}", e);
                        }
                    }
                    _ => { /* Ignore all other events */ }
                }
            }
        })
}

fn handle_connection_registration(
    connection_mapping: &mut HashMap<Entity, Sender<Arc<Event>>>,
    response_channel: &Sender<Arc<Event>>,
    mut command_buffer: &mut CommandBuffer,
) {
    debug!("Registration event incoming");

    // Create a new connection component to properly handle it's state
    let connection = Connection {
        verified: false,
        version_checked: false,
    };
    let connection_entity = command_buffer.start_entity().with_component((connection,)).build();

    // Create mapping so that the event dispatcher knows which response channel to use.
    connection_mapping.insert(connection_entity, response_channel.clone());

    debug!("Registered connection with entity id {}", connection_entity.index());

    send_event(accept_connection_registration(connection_entity), &mut command_buffer);
}

fn handle_request_check_version(
    connection: Option<Entity>,
    packet: &CCheckVersion,
    world: &mut SubWorld,
    mut command_buffer: &mut CommandBuffer,
) -> Result<()> {
    if let Some(connection) = connection {
        let span = info_span!("connection", %connection);
        let _enter = span.enter();

        debug!("Check version event incoming");

        if packet.version.len() != 2 {
            error!(
                "Expected version array to be of length 2 but is {}",
                packet.version.len()
            );
            send_event(reject_check_version(connection), &mut command_buffer);
            return Ok(());
        }

        // TODO properly do the version verification

        trace!(
            "Version 1: {} version 2: {}",
            packet.version[0].value,
            packet.version[1].value
        );

        if let Some(mut component) = world.get_component_mut::<Connection>(connection) {
            component.version_checked = true;
            send_event(accept_check_version(connection), &mut command_buffer);

            if component.verified && component.version_checked {
                // Now that the client is vetted, we will send it some additional information
                handle_post_initialization(connection, &mut command_buffer)?;
            }
        } else {
            error!("Could not find connection component for entity");
            send_event(reject_check_version(connection), &mut command_buffer);
        }
        Ok(())
    } else {
        error!("Entity of the connection for event RequestCheckVersion was not set");
        Err(Error::EntityNotSet)
    }
}

fn handle_request_login_arbiter(
    connection: Option<Entity>,
    packet: &CLoginArbiter,
    world: &mut SubWorld,
    mut command_buffer: &mut CommandBuffer,
) -> Result<()> {
    if let Some(connection) = connection {
        let span = info_span!("connection", %connection);
        let _enter = span.enter();

        debug!(
            "Login arbiter event incoming for master account: {}",
            packet.master_account_name
        );
        let ticket = from_utf8(&packet.ticket)?;
        trace!("Ticket value: {}", ticket);

        // TODO properly handle the request with DB and token verification

        if let Some(mut component) = world.get_component_mut::<Connection>(connection) {
            component.verified = true;
            send_event(accept_login_arbiter(connection, &packet), &mut command_buffer);

            if component.verified && component.version_checked {
                // Now that the client is vetted, we will send it some additional information
                handle_post_initialization(connection, &mut command_buffer)?;
            }
        } else {
            error!("Could not find connection component for entity. Rejecting.");
            send_event(reject_login_arbiter(connection, &packet), &mut command_buffer);
        }
        Ok(())
    } else {
        error!("Entity of the connection for event RequestCheckVersion was not set");
        Err(Error::EntityNotSet)
    }
}

fn handle_post_initialization(connection: Entity, mut command_buffer: &mut CommandBuffer) -> Result<()> {
    send_event(assemble_loading_screen_info(connection), &mut command_buffer);
    // TODO send
    // - S_REMAIN_PLAY_TIME
    // - S_LOGIN_ACCOUNT_INFO
    Ok(())
}

fn assemble_loading_screen_info(connection: Entity) -> Arc<Event> {
    Arc::new(Event::ResponseLoadingScreenControlInfo{
        connection: Some(connection),
        packet: SLoadingScreenControlInfo{
        custom_screen_enabled: false,
        }
    })
}

fn send_event(event: Arc<Event>, command_buffer: &mut CommandBuffer) {
    debug!("Created {} event", event);
    trace!("Event data: {}", event);
    command_buffer
        .start_entity()
        .with_tag((tag::EventKind(EventKind::Response),))
        .with_component((event,))
        .build();
}

fn accept_connection_registration(connection: Entity) -> Arc<Event> {
    Arc::new(Event::ResponseRegisterConnection {
        connection: Some(connection),
    })
}

fn accept_check_version(connection: Entity) -> Arc<Event> {
    Arc::new(Event::ResponseCheckVersion {
        connection: Some(connection),
        packet: SCheckVersion { ok: true },
    })
}

fn reject_check_version(connection: Entity) -> Arc<Event> {
    Arc::new(Event::ResponseCheckVersion {
        connection: Some(connection),
        packet: SCheckVersion { ok: false },
    })
}

fn accept_login_arbiter(connection: Entity, packet: &CLoginArbiter) -> Arc<Event> {
    Arc::new(Event::ResponseLoginArbiter {
        connection: Some(connection),
        packet: SLoginArbiter {
            success: true,
            login_queue: false,
            status: 1,
            unk1: 0,
            region: packet.region,
            pvp_disabled: true,
            unk2: 0,
            unk3: 0,
        },
    })
}

fn reject_login_arbiter(connection: Entity, packet: &CLoginArbiter) -> Arc<Event> {
    Arc::new(Event::ResponseLoginArbiter {
        connection: Some(connection),
        packet: SLoginArbiter {
            success: false,
            login_queue: false,
            status: 0,
            unk1: 0,
            region: packet.region,
            pvp_disabled: false,
            unk2: 0,
            unk3: 0,
        },
    })
}

// TODO Registration test
