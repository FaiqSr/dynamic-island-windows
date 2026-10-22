use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::ui::NotificationKind;

impl From<&'static str> for NotificationKind {
    fn from(icon: &'static str) -> Self {
        match icon {
            "info" | "meeting" | "chat" => Self::Info,
            "warning" => Self::Warning,
            "success" | "battery" | "download" => Self::Success,
            "error" => Self::Error,
            "music" => Self::Music,
            "timer" => Self::Timer,
            _ => Self::Star,
        }
    }
}

pub const LIFETIME: Duration = Duration::from_secs(6);

pub struct Notification {
    pub id: u64,
    pub title: String,
    pub body: String,
    pub icon: NotificationKind,
    pub created: Instant,
}

/// Owned copy of a notification, so the UI can render without borrowing the queue.
#[derive(Clone)]
pub struct NotificationView {
    pub id: u64,
    pub title: String,
    pub body: String,
    pub icon: NotificationKind,
    pub age: f32,
    pub lifetime: f32,
}

impl From<&Notification> for NotificationView {
    fn from(n: &Notification) -> Self {
        Self {
            id: n.id,
            title: n.title.clone(),
            body: n.body.clone(),
            icon: n.icon,
            age: n.age(),
            lifetime: LIFETIME.as_secs_f32(),
        }
    }
}

impl Notification {
    /// Seconds since arrival, used to drive the entry animation.
    pub fn age(&self) -> f32 {
        self.created.elapsed().as_secs_f32()
    }
}

pub struct Notifications {
    queue: VecDeque<Notification>,
    next_id: u64,
}

impl Notifications {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            next_id: 1,
        }
    }

    pub fn push(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        icon: impl Into<NotificationKind>,
    ) {
        let notification = Notification {
            id: self.next_id,
            title: title.into(),
            body: body.into(),
            icon: icon.into(),
            created: Instant::now(),
        };
        self.next_id += 1;
        self.queue.push_front(notification);
        while self.queue.len() > 4 {
            self.queue.pop_back();
        }
    }

    /// Drops the oldest notification once it expires.
    pub fn reap(&mut self) -> bool {
        let previous_len = self.queue.len();
        self.queue.retain(|n| n.created.elapsed() < LIFETIME);
        self.queue.len() != previous_len
    }

    pub fn front(&self) -> Option<&Notification> {
        self.queue.front()
    }

    /// Newest first, as shown in the list panel.
    pub fn items(&self) -> Vec<NotificationView> {
        self.queue.iter().map(NotificationView::from).collect()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn dismiss(&mut self, id: u64) {
        self.queue.retain(|n| n.id != id);
    }

    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_notification_is_front_and_expired_items_are_all_removed() {
        let mut notifications = Notifications::new();
        notifications.push("Old", "older alert", NotificationKind::Info);
        notifications.push("New", "latest alert", NotificationKind::Success);

        assert_eq!(notifications.front().unwrap().title, "New");
        assert_eq!(notifications.items()[0].title, "New");

        for note in &mut notifications.queue {
            note.created = Instant::now() - LIFETIME;
        }
        assert!(notifications.reap());
        assert!(notifications.is_empty());
    }
}
