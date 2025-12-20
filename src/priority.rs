//! Command priority queue and source lockout system.
//!
//! This module provides the infrastructure for managing command priority
//! and source-based lockouts. It ensures that physical controls take
//! precedence over remote commands and prevents command "fighting".
//!
//! # Key Components
//!
//! - [`CommandQueue`]: Priority queue for commands using a binary heap
//! - [`SourceLockout`]: Prevents lower-priority sources from interrupting
//! - [`CommandProcessor`]: Combines queue and lockout for complete processing
//!
//! # Source Lockout
//!
//! When a high-priority source (Physical or above) sends a command, it creates
//! a "lockout" that rejects lower-priority commands for a configurable duration.
//! This prevents scenarios like:
//!
//! - MQTT commands fighting with physical knob control
//! - Web API overriding local user interaction
//!
//! ```rust
//! use rs_trainz::priority::SourceLockout;
//! use rs_trainz::commands::{PrioritizedCommand, ThrottleCommandDyn, CommandSource};
//! use rs_trainz::AnyStrategy;
//! use rs_trainz::traits::Immediate;
//!
//! let mut lockout = SourceLockout::new(2000); // 2 second lockout
//!
//! // Physical command creates lockout
//! let physical = PrioritizedCommand::new(
//!     ThrottleCommandDyn::SetSpeed { target: 0.5, strategy: AnyStrategy::new(Immediate) },
//!     CommandSource::Physical,
//!     0,
//! );
//! assert!(lockout.should_accept(&physical, 0));
//!
//! // MQTT command rejected during lockout
//! let mqtt = PrioritizedCommand::new(
//!     ThrottleCommandDyn::SetSpeed { target: 0.8, strategy: AnyStrategy::new(Immediate) },
//!     CommandSource::Mqtt,
//!     100,
//! );
//! assert!(!lockout.should_accept(&mqtt, 100));
//!
//! // After lockout expires, MQTT accepted
//! let mqtt2 = PrioritizedCommand::new(
//!     ThrottleCommandDyn::SetSpeed { target: 0.3, strategy: AnyStrategy::new(Immediate) },
//!     CommandSource::Mqtt,
//!     2100,
//! );
//! assert!(lockout.should_accept(&mqtt2, 2100));
//! ```
//!
//! # E-Stop Exception
//!
//! E-stop commands always bypass lockout and clear it. This ensures the
//! emergency stop function works regardless of what source is controlling.

use crate::commands::{CommandSource, PrioritizedCommand, ThrottleCommandDyn};
use heapless::binary_heap::{BinaryHeap, Max};

/// Command queue with priority ordering.
///
/// Uses a max-heap to always serve the highest-priority command first.
/// When full, lower-priority commands can be displaced by higher-priority ones.
///
/// # Capacity
///
/// The queue has a fixed capacity `N` (const generic). When full:
/// - Higher priority commands displace the lowest priority item
/// - Equal or lower priority commands are rejected
///
/// # Example
///
/// ```rust
/// use rs_trainz::priority::CommandQueue;
/// use rs_trainz::commands::{PrioritizedCommand, ThrottleCommandDyn, CommandSource};
/// use rs_trainz::AnyStrategy;
/// use rs_trainz::traits::Immediate;
///
/// let mut queue: CommandQueue<4> = CommandQueue::new();
///
/// let cmd = PrioritizedCommand::new(
///     ThrottleCommandDyn::SetSpeed { target: 0.5, strategy: AnyStrategy::new(Immediate) },
///     CommandSource::Physical,
///     0,
/// );
/// assert!(queue.push(cmd));
///
/// // Pop returns highest priority first
/// let popped = queue.pop().unwrap();
/// assert_eq!(popped.source, CommandSource::Physical);
/// ```
pub struct CommandQueue<const N: usize> {
    heap: BinaryHeap<PrioritizedCommand, Max, N>,
}

impl<const N: usize> CommandQueue<N> {
    /// Creates a new empty command queue with capacity N.
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    /// Push a command onto the queue
    ///
    /// If the queue is full, only accepts if higher priority than lowest in queue.
    /// When accepted, the lowest priority item is dropped to make room.
    #[must_use]
    pub fn push(&mut self, cmd: PrioritizedCommand) -> bool {
        if self.heap.len() < self.heap.capacity() {
            return self.heap.push(cmd).is_ok();
        }

        // Queue full - drain heap to find minimum and decide whether to accept
        let new_priority = cmd.priority();

        // Drain all items to find the minimum priority
        let mut items: heapless::Vec<PrioritizedCommand, N> = heapless::Vec::new();
        while let Some(item) = self.heap.pop() {
            let _ = items.push(item);
        }

        // Find minimum priority
        let min_priority = items.iter().map(|c| c.priority()).min();

        match min_priority {
            Some(min_p) if new_priority > min_p => {
                // New command has higher priority than minimum - accept it
                // Rebuild heap without ONE lowest priority item
                let mut dropped_one = false;
                for item in items {
                    if !dropped_one && item.priority() == min_p {
                        dropped_one = true; // Drop this lowest priority item
                    } else {
                        let _ = self.heap.push(item);
                    }
                }
                self.heap.push(cmd).is_ok()
            }
            _ => {
                // New command has equal or lower priority - reject and restore heap
                for item in items {
                    let _ = self.heap.push(item);
                }
                false
            }
        }
    }

    /// Pop the highest priority command
    pub fn pop(&mut self) -> Option<PrioritizedCommand> {
        self.heap.pop()
    }

    /// Peek at the highest priority command without removing it
    pub fn peek(&self) -> Option<&PrioritizedCommand> {
        self.heap.peek()
    }

    /// Clear all commands below a certain source priority
    pub fn clear_below(&mut self, source: CommandSource) {
        let mut temp: BinaryHeap<PrioritizedCommand, Max, N> = BinaryHeap::new();
        while let Some(cmd) = self.heap.pop() {
            if cmd.priority().0 >= source {
                let _ = temp.push(cmd);
            }
        }
        self.heap = temp;
    }

    /// Clear all commands
    pub fn clear(&mut self) {
        while self.heap.pop().is_some() {}
    }

    /// Returns the number of commands in the queue.
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Returns true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Returns true if the queue is at capacity.
    pub fn is_full(&self) -> bool {
        self.heap.len() == self.heap.capacity()
    }
}

impl<const N: usize> Default for CommandQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Source lockout system - prevents lower priority sources from interrupting.
///
/// When a high-priority source (Physical or above) sends a command, the lockout
/// system blocks lower-priority sources for a configurable duration. This
/// prevents "command fighting" between local and remote control.
///
/// # Lockout Rules
///
/// - Sources below [`Physical`](CommandSource::Physical) don't create lockouts
/// - Same or higher priority commands extend the lockout timer
/// - E-stop commands always bypass and clear the lockout
/// - Lockout expires after the configured duration with no high-priority commands
///
/// # Example
///
/// ```rust
/// use rs_trainz::priority::SourceLockout;
///
/// let mut lockout = SourceLockout::new(2000); // 2 second lockout
///
/// // Check status
/// if let Some(status) = lockout.status(500) {
///     println!("Locked by {:?}, {} ms remaining", status.source, status.remaining_ms);
/// }
/// ```
pub struct SourceLockout {
    active_source: Option<CommandSource>,
    lockout_until_ms: u64,
    lockout_duration_ms: u64,
}

impl SourceLockout {
    /// Create a new lockout system
    ///
    /// # Arguments
    /// * `lockout_duration_ms` - How long a source maintains priority after its last command
    pub fn new(lockout_duration_ms: u64) -> Self {
        Self {
            active_source: None,
            lockout_until_ms: 0,
            lockout_duration_ms,
        }
    }

    /// Check if a command should be accepted
    ///
    /// Returns true if the command is accepted, false if rejected due to lockout
    #[must_use]
    pub fn should_accept(&mut self, cmd: &PrioritizedCommand, now_ms: u64) -> bool {
        // E-stop always accepted, clears lockout
        if cmd.command.is_estop() {
            self.active_source = None;
            return true;
        }

        // Check if lockout expired
        if now_ms >= self.lockout_until_ms {
            self.active_source = None;
        }

        match self.active_source {
            None => {
                // No lockout - accept and maybe start one
                if cmd.source >= CommandSource::Physical {
                    self.active_source = Some(cmd.source);
                    self.lockout_until_ms = now_ms + self.lockout_duration_ms;
                }
                true
            }
            Some(locked_source) => {
                if cmd.source >= locked_source {
                    // Same or higher priority - accept and extend lockout
                    self.active_source = Some(cmd.source);
                    self.lockout_until_ms = now_ms + self.lockout_duration_ms;
                    true
                } else {
                    // Lower priority - reject during lockout
                    false
                }
            }
        }
    }

    /// Clear the current lockout
    pub fn clear(&mut self) {
        self.active_source = None;
    }

    /// Get the current lockout status
    pub fn status(&self, now_ms: u64) -> Option<LockoutStatus> {
        if now_ms >= self.lockout_until_ms {
            return None;
        }
        self.active_source.map(|source| LockoutStatus {
            source,
            expires_ms: self.lockout_until_ms,
            remaining_ms: self.lockout_until_ms.saturating_sub(now_ms),
        })
    }
}

/// Information about an active lockout.
///
/// Returned by [`SourceLockout::status`] when a lockout is active.
/// Useful for UI feedback showing when remote control will be available.
#[derive(Clone, Debug)]
pub struct LockoutStatus {
    /// The source that holds the lockout.
    pub source: CommandSource,
    /// Timestamp when the lockout expires (milliseconds since start).
    pub expires_ms: u64,
    /// Time remaining until lockout expires (milliseconds).
    pub remaining_ms: u64,
}

/// Combined command processor with queue and lockout.
///
/// This is the main entry point for command processing. It combines
/// [`CommandQueue`] and [`SourceLockout`] into a single interface.
///
/// # Usage
///
/// ```rust
/// use rs_trainz::priority::CommandProcessor;
/// use rs_trainz::commands::{PrioritizedCommand, ThrottleCommandDyn, CommandSource};
/// use rs_trainz::AnyStrategy;
/// use rs_trainz::traits::Immediate;
///
/// let mut processor: CommandProcessor<8> = CommandProcessor::new(2000);
///
/// // Submit commands
/// let cmd = PrioritizedCommand::new(
///     ThrottleCommandDyn::SetSpeed { target: 0.5, strategy: AnyStrategy::new(Immediate) },
///     CommandSource::Physical,
///     0,
/// );
/// processor.submit(cmd, 0);
///
/// // Process next command
/// if let Some(command) = processor.process_next() {
///     // Execute command...
/// }
/// ```
pub struct CommandProcessor<const N: usize> {
    queue: CommandQueue<N>,
    lockout: SourceLockout,
}

impl<const N: usize> CommandProcessor<N> {
    /// Creates a new command processor with the given lockout duration.
    pub fn new(lockout_duration_ms: u64) -> Self {
        Self {
            queue: CommandQueue::new(),
            lockout: SourceLockout::new(lockout_duration_ms),
        }
    }

    /// Submit a command for processing
    ///
    /// Returns true if accepted into queue, false if rejected
    #[must_use]
    pub fn submit(&mut self, cmd: PrioritizedCommand, now_ms: u64) -> bool {
        if self.lockout.should_accept(&cmd, now_ms) {
            self.queue.push(cmd)
        } else {
            false
        }
    }

    /// Process the next command
    pub fn process_next(&mut self) -> Option<ThrottleCommandDyn> {
        self.queue.pop().map(|pc| pc.command)
    }

    /// Clear all pending commands after an e-stop
    pub fn clear_after_estop(&mut self) {
        self.queue.clear_below(CommandSource::Emergency);
        self.lockout.clear();
    }

    /// Clear all pending commands
    pub fn clear_all(&mut self) {
        self.queue.clear();
        self.lockout.clear();
    }

    /// Returns the number of commands in the queue.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Returns the current lockout status, if any.
    pub fn lockout_status(&self, now_ms: u64) -> Option<LockoutStatus> {
        self.lockout.status(now_ms)
    }
}

impl<const N: usize> Default for CommandProcessor<N> {
    fn default() -> Self {
        Self::new(2000) // 2 second default lockout
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy_dyn::AnyStrategy;
    use crate::traits::Immediate;

    fn make_cmd(source: CommandSource, timestamp: u64) -> PrioritizedCommand {
        PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.5,
                strategy: AnyStrategy::new(Immediate),
            },
            source,
            timestamp,
        )
    }

    fn make_estop(source: CommandSource, timestamp: u64) -> PrioritizedCommand {
        PrioritizedCommand::new(ThrottleCommandDyn::EmergencyStop, source, timestamp)
    }

    // === CommandQueue Tests ===
    #[test]
    fn queue_new_is_empty() {
        let q: CommandQueue<4> = CommandQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn queue_push_and_pop() {
        let mut q: CommandQueue<4> = CommandQueue::new();
        assert!(q.push(make_cmd(CommandSource::Mqtt, 0)));
        assert_eq!(q.len(), 1);
        assert!(!q.is_empty());
        assert!(q.pop().is_some());
        assert!(q.is_empty());
    }

    #[test]
    fn queue_priority_ordering() {
        let mut q: CommandQueue<4> = CommandQueue::new();
        let _ = q.push(make_cmd(CommandSource::Mqtt, 0));
        let _ = q.push(make_cmd(CommandSource::Physical, 0));
        let _ = q.push(make_cmd(CommandSource::WebApi, 0));

        // Should pop in priority order (highest first)
        assert_eq!(q.pop().unwrap().source, CommandSource::Physical);
        assert_eq!(q.pop().unwrap().source, CommandSource::WebApi);
        assert_eq!(q.pop().unwrap().source, CommandSource::Mqtt);
    }

    #[test]
    fn queue_full_rejects_low_priority() {
        let mut q: CommandQueue<2> = CommandQueue::new();
        let _ = q.push(make_cmd(CommandSource::Physical, 0));
        let _ = q.push(make_cmd(CommandSource::WebApi, 0));
        assert!(q.is_full());

        // Queue full - Mqtt (lowest) should be rejected
        assert!(!q.push(make_cmd(CommandSource::Mqtt, 0)));
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn queue_full_accepts_higher_priority() {
        let mut q: CommandQueue<2> = CommandQueue::new();
        let _ = q.push(make_cmd(CommandSource::Mqtt, 0));
        let _ = q.push(make_cmd(CommandSource::WebApi, 0));

        // Should accept Physical and drop lowest (Mqtt)
        assert!(q.push(make_cmd(CommandSource::Physical, 0)));
        assert_eq!(q.len(), 2);

        // Verify Mqtt was dropped
        let first = q.pop().unwrap();
        let second = q.pop().unwrap();
        assert!(first.source != CommandSource::Mqtt);
        assert!(second.source != CommandSource::Mqtt);
    }

    #[test]
    fn queue_peek_does_not_remove() {
        let mut q: CommandQueue<4> = CommandQueue::new();
        let _ = q.push(make_cmd(CommandSource::Physical, 0));

        assert!(q.peek().is_some());
        assert_eq!(q.peek().unwrap().source, CommandSource::Physical);
        assert_eq!(q.len(), 1); // Still there
    }

    #[test]
    fn queue_clear_removes_all() {
        let mut q: CommandQueue<4> = CommandQueue::new();
        let _ = q.push(make_cmd(CommandSource::Mqtt, 0));
        let _ = q.push(make_cmd(CommandSource::Physical, 0));
        let _ = q.push(make_cmd(CommandSource::WebApi, 0));

        q.clear();
        assert!(q.is_empty());
    }

    #[test]
    fn queue_clear_below_keeps_higher() {
        let mut q: CommandQueue<4> = CommandQueue::new();
        let _ = q.push(make_cmd(CommandSource::Mqtt, 0));
        let _ = q.push(make_cmd(CommandSource::WebApi, 0));
        let _ = q.push(make_cmd(CommandSource::Physical, 0));
        let _ = q.push(make_cmd(CommandSource::Fault, 0));

        q.clear_below(CommandSource::Physical);

        assert_eq!(q.len(), 2);
        let sources: Vec<_> = (0..2).filter_map(|_| q.pop()).map(|c| c.source).collect();
        assert!(sources.contains(&CommandSource::Physical));
        assert!(sources.contains(&CommandSource::Fault));
    }

    #[test]
    fn queue_default_is_empty() {
        let q: CommandQueue<4> = CommandQueue::default();
        assert!(q.is_empty());
    }

    // === SourceLockout Tests ===
    #[test]
    fn lockout_accepts_first_command() {
        let mut lockout = SourceLockout::new(2000);
        let cmd = make_cmd(CommandSource::Mqtt, 0);
        assert!(lockout.should_accept(&cmd, 0));
    }

    #[test]
    fn lockout_mqtt_does_not_create_lockout() {
        let mut lockout = SourceLockout::new(2000);
        let mqtt = make_cmd(CommandSource::Mqtt, 0);
        let _ = lockout.should_accept(&mqtt, 0);

        // Another Mqtt should still be accepted (no lockout from Mqtt)
        let mqtt2 = make_cmd(CommandSource::Mqtt, 100);
        assert!(lockout.should_accept(&mqtt2, 100));

        // Status should be None (no lockout active)
        assert!(lockout.status(100).is_none());
    }

    #[test]
    fn lockout_physical_creates_lockout() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&physical, 0);

        // Mqtt should be rejected during lockout
        let mqtt = make_cmd(CommandSource::Mqtt, 100);
        assert!(!lockout.should_accept(&mqtt, 100));
    }

    #[test]
    fn lockout_same_priority_accepted() {
        let mut lockout = SourceLockout::new(2000);
        let p1 = make_cmd(CommandSource::Physical, 0);
        let p2 = make_cmd(CommandSource::Physical, 100);

        let _ = lockout.should_accept(&p1, 0);
        assert!(lockout.should_accept(&p2, 100)); // Same priority OK
    }

    #[test]
    fn lockout_higher_priority_accepted() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let fault = make_cmd(CommandSource::Fault, 100);

        let _ = lockout.should_accept(&physical, 0);
        assert!(lockout.should_accept(&fault, 100)); // Higher OK
    }

    #[test]
    fn lockout_expires() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&physical, 0);

        // After 2000ms, lockout should expire
        let mqtt = make_cmd(CommandSource::Mqtt, 2001);
        assert!(lockout.should_accept(&mqtt, 2001));
    }

    #[test]
    fn lockout_estop_always_accepted() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&physical, 0);

        // E-stop from Mqtt should still work
        let estop = make_estop(CommandSource::Mqtt, 100);
        assert!(lockout.should_accept(&estop, 100));
    }

    #[test]
    fn lockout_estop_clears_lockout() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&physical, 0);

        let estop = make_estop(CommandSource::Mqtt, 100);
        let _ = lockout.should_accept(&estop, 100);

        // Lockout should be cleared, Mqtt commands should work
        let mqtt = make_cmd(CommandSource::Mqtt, 200);
        assert!(lockout.should_accept(&mqtt, 200));
    }

    #[test]
    fn lockout_status_during_lockout() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&physical, 0);

        let status = lockout.status(1000).unwrap();
        assert_eq!(status.source, CommandSource::Physical);
        assert_eq!(status.expires_ms, 2000);
        assert_eq!(status.remaining_ms, 1000);
    }

    #[test]
    fn lockout_status_none_when_expired() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&physical, 0);

        assert!(lockout.status(3000).is_none());
    }

    #[test]
    fn lockout_clear_removes_lockout() {
        let mut lockout = SourceLockout::new(2000);
        let physical = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&physical, 0);

        lockout.clear();

        // Mqtt should now be accepted
        let mqtt = make_cmd(CommandSource::Mqtt, 100);
        assert!(lockout.should_accept(&mqtt, 100));
    }

    #[test]
    fn lockout_extends_on_new_command() {
        let mut lockout = SourceLockout::new(2000);
        let p1 = make_cmd(CommandSource::Physical, 0);
        let _ = lockout.should_accept(&p1, 0);

        // Second command at 1500ms should extend lockout to 3500ms
        let p2 = make_cmd(CommandSource::Physical, 1500);
        let _ = lockout.should_accept(&p2, 1500);

        let status = lockout.status(2000).unwrap();
        assert_eq!(status.expires_ms, 3500);
    }

    // === CommandProcessor Tests ===
    #[test]
    fn processor_submit_and_process() {
        let mut proc: CommandProcessor<4> = CommandProcessor::new(2000);
        let cmd = make_cmd(CommandSource::Mqtt, 0);

        assert!(proc.submit(cmd, 0));
        assert_eq!(proc.queue_len(), 1);

        let processed = proc.process_next();
        assert!(processed.is_some());
        assert_eq!(proc.queue_len(), 0);
    }

    #[test]
    fn processor_respects_lockout() {
        let mut proc: CommandProcessor<4> = CommandProcessor::new(2000);

        // Physical creates lockout
        let _ = proc.submit(make_cmd(CommandSource::Physical, 0), 0);
        let _ = proc.process_next();

        // Mqtt should be rejected
        assert!(!proc.submit(make_cmd(CommandSource::Mqtt, 100), 100));
    }

    #[test]
    fn processor_clear_after_estop() {
        let mut proc: CommandProcessor<4> = CommandProcessor::new(2000);
        let _ = proc.submit(make_cmd(CommandSource::Mqtt, 0), 0);
        let _ = proc.submit(make_cmd(CommandSource::Physical, 0), 0);

        proc.clear_after_estop();

        // Only Emergency-level commands should remain (none in this case)
        assert!(proc.process_next().is_none());
    }

    #[test]
    fn processor_clear_all() {
        let mut proc: CommandProcessor<4> = CommandProcessor::new(2000);
        let _ = proc.submit(make_cmd(CommandSource::Physical, 0), 0);
        let _ = proc.submit(make_cmd(CommandSource::Fault, 0), 0);

        proc.clear_all();

        assert!(proc.process_next().is_none());
        assert_eq!(proc.queue_len(), 0);
    }

    #[test]
    fn processor_lockout_status() {
        let mut proc: CommandProcessor<4> = CommandProcessor::new(2000);
        let _ = proc.submit(make_cmd(CommandSource::Physical, 0), 0);

        let status = proc.lockout_status(500).unwrap();
        assert_eq!(status.source, CommandSource::Physical);
    }

    #[test]
    fn processor_default_has_2s_lockout() {
        let mut proc: CommandProcessor<4> = CommandProcessor::default();
        let _ = proc.submit(make_cmd(CommandSource::Physical, 0), 0);

        // Should still be locked at 1999ms
        assert!(!proc.submit(make_cmd(CommandSource::Mqtt, 0), 1999));

        // Should be unlocked at 2001ms
        assert!(proc.submit(make_cmd(CommandSource::Mqtt, 0), 2001));
    }
}
