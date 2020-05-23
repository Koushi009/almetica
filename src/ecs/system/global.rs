/// All systems used by the global world
mod connection_manager;
mod local_world_manager;
mod settings_manager;
mod user_manager;
mod user_spawner;

pub use connection_manager::connection_manager_system;
pub use local_world_manager::local_world_manager_system;
pub use settings_manager::settings_manager_system;
pub use user_manager::user_manager_system;
pub use user_spawner::user_spawner_system;

use crate::ecs::component::Connection;
use crate::ecs::message::EcsMessage;
use crate::ecs::system::send_message;
use tracing::{debug, error};

// TODO refactor this and the local version

/// Send an outgoing packet message. This function can't be used by "Special Messages".
pub fn send_packet_message<'a, T>(message: EcsMessage, connections: T)
where
    T: shipyard::Get<Out = &'a Connection>,
{
    if let Some(connection_id) = message.connection_id() {
        if let Ok(connection) = connections.try_get(connection_id) {
            send_message(message, &connection.channel);
        } else {
            debug!("Couldn't find connection: {:?}", connection_id);
        }
    } else {
        error!("Message didn't had a global world ID attached");
    }
}
