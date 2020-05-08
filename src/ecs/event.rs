/// Events that are handled / emitted by the ECS.
///
/// Events are only to be used for communication between
/// ECS and connections (and ECS to ECS). They are not to be used for ECS internal communication.
/// For this, you should use other components.
///
/// There are packet and system events.
/// System Events are special events that don't send out a packet to the client and are normally
/// handling the state between the server systems.
///
/// A event always has a target: Global ECS, local ECS or a connection.
///
/// Messages from the connections  to the ECS are always requests.
/// Messages from the ECS to the Connections are always responses.
/// Messages between the ECS can be either request or response.
///
/// Network connections and ECS have a channel to write events into.
///
use crate::protocol::opcode::Opcode;
use crate::protocol::packet::*;
use crate::protocol::serde::{from_vec, to_vec};
use crate::{AlmeticaError, Result};
use anyhow::bail;
use async_std::sync::Sender;
use shipyard::*;
use std::fmt;

/// ECS events. We use `Box` so that we don't need to copy packet data around.
pub type EcsEvent = Box<Event>;

/// The target of the event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventTarget {
    Global,
    Local,
    Connection,
}

macro_rules! assemble_event {
    (
    Packet Events {
        $($p_ty:ident{packet: $p_packet_type:ty $(, $p_arg_name:ident: $p_arg_type:ty)*}, $p_opcode:ident, $p_target:ident;)*
    }
    System Events {
        $($e_ty:ident{$($e_arg_name:ident: $e_arg_type:ty)*}, $e_target:ident;)*
    }
    ) => {
        /// Event enum for all events.
        #[derive(Clone, Debug)]
        pub enum Event {
            RequestRegisterConnection{response_channel: Sender<Box<Event>>},
            $($p_ty {connection_id: EntityId, packet: $p_packet_type $(,$p_arg_name: $p_arg_type)*},)*
            $($e_ty {connection_id: EntityId, $($e_arg_name: $e_arg_type),*},)*
        }

        impl Event {
            /// Creates a new Request/Response event for the given opcode & packet data.
            pub fn new_from_packet(connection_id: EntityId, opcode: Opcode, packet_data: Vec<u8>) -> Result<Event> {
                match opcode {
                    $(Opcode::$p_opcode => {
                        let packet = from_vec(packet_data)?;
                        Ok(Event::$p_ty{connection_id: connection_id, packet})
                    },)*
                    _ => bail!(AlmeticaError::NoEventMappingForPacket),
                }
            }

            /// Get the connection id of a packet event.
            pub fn connection_id(&self) -> Option<EntityId> {
                match self {
                    Event::RequestRegisterConnection{..} => None,
                    $(Event::$p_ty{connection_id,..} => Some(*connection_id),)*
                    $(Event::$e_ty{connection_id,..} => Some(*connection_id),)*
                }
            }

            /// Get the data from a packet event.
            pub fn data(&self) -> Result<Option<Vec<u8>>> {
                match self {
                    $(Event::$p_ty{packet, ..} => {
                        let data = to_vec(packet)?;
                        Ok(Some(data))
                    },)*
                    _ => Ok(None),
                }
            }

            /// Get the opcode from a packet event.
            pub fn opcode(&self) -> Option<Opcode> {
                match self {
                    $(Event::$p_ty{..} => {
                        Some(Opcode::$p_opcode)
                    },)*
                    _ => None,
                }
            }

            /// Get the target of the event (global world / local world / connection).
            pub fn target(&self) -> EventTarget {
                match self {
                    Event::RequestRegisterConnection{..} => EventTarget::Global,
                    $(Event::$p_ty{..} => EventTarget::$p_target,)*
                    $(Event::$e_ty{..} => EventTarget::$e_target,)*
                }
            }
        }

        impl fmt::Display for Event {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                match self {
                    Event::RequestRegisterConnection{..} => write!(f, "{}", stringify!(RequestRegisterConnection)),
                    $(Event::$p_ty{..} => write!(f, "{}", stringify!($p_ty)),)*
                    $(Event::$e_ty{..} => write!(f, "{}", stringify!($e_ty)),)*
                }
            }
        }
    };
}

assemble_event! {
    Packet Events {
        RequestLoginArbiter{packet: CLoginArbiter}, C_LOGIN_ARBITER, Global;
        ResponseLoginArbiter{packet: SLoginArbiter}, S_LOGIN_ARBITER, Connection;
        RequestCheckVersion{packet: CCheckVersion}, C_CHECK_VERSION, Global;
        ResponseCheckVersion{packet: SCheckVersion}, S_CHECK_VERSION, Connection;
        ResponseLoadingScreenControlInfo{packet: SLoadingScreenControlInfo}, S_LOADING_SCREEN_CONTROL_INFO, Connection;
        ResponseRemainPlayTime{packet: SRemainPlayTime}, S_REMAIN_PLAY_TIME, Connection;
        ResponseLoginAccountInfo{packet: SLoginAccountInfo}, S_LOGIN_ACCOUNT_INFO, Connection;
        RequestSetVisibleRange{packet: CSetVisibleRange}, C_SET_VISIBLE_RANGE, Global;
        RequestGetUserList{packet: CGetUserList}, C_GET_USER_LIST, Global;
        ResponseGetUserList{packet: SGetUserList}, S_GET_USER_LIST, Global;
        RequestPong{packet: CPong}, C_PONG, Global;
        ResponsePing{packet: SPing}, S_PING, Connection;
        RequestCanCreateUser{packet: CCanCreateUser}, C_CAN_CREATE_USER, Global;
        ResponseCanCreateUser{packet: SCanCreateUser}, S_CAN_CREATE_USER, Connection;
        RequestCheckUserName{packet: CCheckUserName}, C_CHECK_USERNAME, Global;
        ResponseCheckUserName{packet: SCheckUserName}, S_CHECK_USERNAME, Connection;
        RequestCreateUser{packet: CCreateUser}, C_CREATE_USER, Global;
        ResponseCreateUser{packet: SCreateUser}, S_CREATE_USER, Connection;
    }
    System Events {
        // The connection will get it's EntityId returned with this message after registration.
        ResponseRegisterConnection{}, Connection;
        // The connection will be dropped after it receives this message.
        ResponseDropConnection{}, Connection;
    }
}

#[cfg(test)]
mod tests {
    use async_std::sync::channel;
    use shipyard::*;

    use crate::model::Region;
    use crate::protocol::opcode::Opcode;

    use super::*;

    #[test]
    fn test_opcode_mapping() -> Result<()> {
        let entity = World::new().borrow::<EntitiesViewMut>().add_entity((), ());

        let data = vec![
            0x2, 0x0, 0x8, 0x0, 0x8, 0x0, 0x14, 0x0, 0x0, 0x0, 0x0, 0x0, 0x1d, 0x8a, 0x5, 0x0,
            0x14, 0x0, 0x0, 0x0, 0x1, 0x0, 0x0, 0x0, 0xce, 0x7b, 0x5, 0x0,
        ];
        let event = Event::new_from_packet(entity, Opcode::C_CHECK_VERSION, data)?;
        if let Event::RequestCheckVersion {
            connection_id: entity_id,
            packet,
        } = event
        {
            assert_eq!(entity, entity_id);
            assert_eq!(packet.version[0].index, 0);
            assert_eq!(packet.version[0].value, 363_037);
            assert_eq!(packet.version[1].index, 1);
            assert_eq!(packet.version[1].value, 359_374);
        } else {
            panic!("New didn't returned the right event.");
        }
        Ok(())
    }

    #[test]
    fn test_target_global() -> Result<()> {
        let entity = World::new().borrow::<EntitiesViewMut>().add_entity((), ());
        let org = Event::RequestLoginArbiter {
            connection_id: entity,
            packet: CLoginArbiter {
                master_account_name: "test".to_string(),
                ticket: vec![],
                unk1: 0,
                unk2: 0,
                region: Region::Europe,
                patch_version: 0,
            },
        };
        assert_eq!(org.target(), EventTarget::Global);
        Ok(())
    }

    #[test]
    fn test_target_connection() -> Result<()> {
        let entity = World::new().borrow::<EntitiesViewMut>().add_entity((), ());
        let org = Event::ResponseCheckVersion {
            connection_id: entity,
            packet: SCheckVersion { ok: true },
        };
        assert_eq!(org.target(), EventTarget::Connection);
        Ok(())
    }

    #[test]
    fn test_event_opcode_some() -> Result<()> {
        let entity = World::new().borrow::<EntitiesViewMut>().add_entity((), ());
        let org = Event::ResponseCheckVersion {
            connection_id: entity,
            packet: SCheckVersion { ok: true },
        };
        assert_eq!(org.opcode(), Some(Opcode::S_CHECK_VERSION));
        Ok(())
    }

    #[test]
    fn test_event_opcode_none() -> Result<()> {
        let (response_channel, _) = channel(1);
        let org = Event::RequestRegisterConnection { response_channel };

        assert_eq!(org.opcode(), None);
        Ok(())
    }

    #[test]
    fn test_event_connection_some() -> Result<()> {
        let entity = World::new().borrow::<EntitiesViewMut>().add_entity((), ());
        let org = Event::ResponseCheckVersion {
            connection_id: entity,
            packet: SCheckVersion { ok: true },
        };
        assert_eq!(org.connection_id(), Some(entity));
        Ok(())
    }

    #[test]
    fn test_event_connection_none() -> Result<()> {
        let (response_channel, _) = channel(1);
        let org = Event::RequestRegisterConnection { response_channel };

        assert_eq!(org.connection_id(), None);
        Ok(())
    }
}
