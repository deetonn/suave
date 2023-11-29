use std::io;

use libc::mmsghdr;
use libc::msgget;
use libc::msghdr;
use libc::recvmsg as msgrecv;
use libc::sendmmsg as msgsnd;

/// The type returned by the kernel that represents the id of a message queue.
pub type MsgQueueId = i32;

/// Create/Get a message queue. This is public because it wraps the libc function
/// and could be useful for specific implementations.
///
/// ## Example
/// ```
/// let key = get_key();
/// let queue_id = queue_get(key, 0)
/// ```
pub fn queue_get(key: libc::key_t, flags: i32) -> Result<MsgQueueId, io::Error> {
    let queue = unsafe { msgget(key, flags) };
    if queue > 0 {
        return Ok(queue);
    }
    Err(io::Error::last_os_error())
}

/// A message sent to the systems message queue.
pub struct Message {
    text: String,
}

impl Message {
    pub fn new(text: String) -> Self {
        Self { text }
    }

    pub fn from_object(object: &impl ToString) -> Self {
        Self {
            text: object.to_string(),
        }
    }
}

pub fn message_send(id: MsgQueueId, message: Message) -> i32 {
    todo!()
}

pub struct MessageQueue {}
