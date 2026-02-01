// SC Monitor Hook DLL - Network packet monitoring
// Hooks: recv, send, sendto, recvfrom, connect, closesocket

use minhook::MinHook;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use windows::core::PCSTR;
use windows::Win32::Foundation::{BOOL, HANDLE, HMODULE, TRUE};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleHandleA, GetProcAddress};
use windows::Win32::System::Memory::{
    CreateFileMappingA, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{CreateThread, THREAD_CREATION_FLAGS};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

// ============================================================================
// Types for Winsock
// ============================================================================

type SOCKET = usize;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SockAddrIn {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: u32,
    pub sin_zero: [u8; 8],
}

// WSABUF structure for WSA functions
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WsaBuf {
    pub len: u32,
    pub buf: *mut u8,
}

// Function type definitions
type RecvFn = unsafe extern "system" fn(SOCKET, *mut u8, i32, i32) -> i32;
type SendFn = unsafe extern "system" fn(SOCKET, *const u8, i32, i32) -> i32;
type SendToFn = unsafe extern "system" fn(SOCKET, *const u8, i32, i32, *const SockAddrIn, i32) -> i32;
type RecvFromFn = unsafe extern "system" fn(SOCKET, *mut u8, i32, i32, *mut SockAddrIn, *mut i32) -> i32;
type ConnectFn = unsafe extern "system" fn(SOCKET, *const SockAddrIn, i32) -> i32;
type CloseSocketFn = unsafe extern "system" fn(SOCKET) -> i32;

// WSA function types
type WSASendToFn = unsafe extern "system" fn(
    SOCKET, *const WsaBuf, u32, *mut u32, u32, *const SockAddrIn, i32, *mut c_void, *mut c_void
) -> i32;
type WSARecvFromFn = unsafe extern "system" fn(
    SOCKET, *const WsaBuf, u32, *mut u32, *mut u32, *mut SockAddrIn, *mut i32, *mut c_void, *mut c_void
) -> i32;

// File system function types (map scan skip)
type CreateFileWFn = unsafe extern "system" fn(
    *const u16, u32, u32, *mut c_void, u32, u32, *mut c_void
) -> *mut c_void;
type CreateFileAFn = unsafe extern "system" fn(
    *const u8, u32, u32, *mut c_void, u32, u32, *mut c_void
) -> *mut c_void;

// ============================================================================
// Original Function Pointers
// ============================================================================

static ORIGINAL_RECV: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_SEND: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_SENDTO: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_RECVFROM: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_CONNECT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_CLOSESOCKET: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_WSASENDTO: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_WSARECVFROM: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_CREATE_FILE_W: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_CREATE_FILE_A: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

// ============================================================================
// Global State
// ============================================================================

static HOOKS_INSTALLED: AtomicBool = AtomicBool::new(false);
static COMMAND_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);

const SHARED_MEMORY_NAME: &[u8] = b"Local\\SCMonitorPacketData\0";
const AUTOBAN_MEMORY_NAME: &[u8] = b"Local\\SCMonitorAutoBan\0";
const ROOMUSERS_MEMORY_NAME: &[u8] = b"Local\\SCMonitorRoomUsers\0";

#[repr(C)]
pub struct SharedPacketData {
    pub packet_count: u32,
    pub last_packet_type: u32,
    pub last_packet_size: u32,
    pub last_socket: u64,
    pub last_dest_ip: u32,
    pub last_dest_port: u16,
    pub has_kick_data: u32,
}

// Auto-ban shared memory structure
pub const MAX_BAN_TARGETS: usize = 10;
pub const MAX_BATTLETAG_LEN: usize = 64;
pub const MAX_MEMO_LEN: usize = 128;

#[repr(C)]
pub struct AutoBanConfig {
    pub version: u32,                                    // Structure version (2)
    pub enabled: u32,                                    // 0 = disabled, 1 = enabled
    pub target_count: u32,                               // Number of active targets
    pub reserved: u32,                                   // Padding
    pub targets: [[u8; MAX_BATTLETAG_LEN]; MAX_BAN_TARGETS],  // UTF-8 battletags
    pub memos: [[u8; MAX_MEMO_LEN]; MAX_BAN_TARGETS],    // Memo/reason for each target
    pub total_kicks: u32,                                // Statistics
    pub successful_kicks: u32,
    pub last_kick_time: u32,                             // Unix timestamp
    pub last_kicked_battletag: [u8; MAX_BATTLETAG_LEN],  // Last kicked player
}

// Global pointer to auto-ban shared memory
static AUTOBAN_CONFIG: AtomicPtr<AutoBanConfig> = AtomicPtr::new(std::ptr::null_mut());

// Room users shared memory structure
pub const MAX_ROOM_USERS: usize = 100;
pub const MAX_NICKNAME_LEN: usize = 32;
pub const MAX_IP_LEN: usize = 16;  // "255.255.255.255\0"

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RoomUser {
    pub active: u32,                              // 1 if slot is used, 0 if empty
    pub player_id: u32,                           // Player ID
    pub nickname: [u8; MAX_NICKNAME_LEN],         // Player nickname from PNM: packet
    pub battletag: [u8; MAX_BATTLETAG_LEN],       // Battletag (e.g., "Name#12345")
    pub ip_address: [u8; MAX_IP_LEN],             // IP address from WSARECVFROM
    pub port: u16,                                // Port from WSARECVFROM
    pub flags: u16,                               // Reserved for future use
}

#[repr(C)]
pub struct RoomUsersData {
    pub version: u32,                             // Structure version
    pub user_count: u32,                          // Number of active users
    pub last_update: u32,                         // Unix timestamp of last update
    pub reserved: u32,                            // Padding
    pub users: [RoomUser; MAX_ROOM_USERS],        // User slots
}

// Global pointer to room users shared memory
static ROOM_USERS: AtomicPtr<RoomUsersData> = AtomicPtr::new(std::ptr::null_mut());

// Room info shared memory structure
pub const MAX_ROOM_NAME_LEN: usize = 64;
pub const MAX_MAP_NAME_LEN: usize = 128;
pub const MAX_HOST_NAME_LEN: usize = 64;
pub const MAX_FORCE_NAME_LEN: usize = 64;
pub const MAX_FORCES: usize = 4;
pub const MAX_ROOM_ID_LEN: usize = 16;
pub const ROOMINFO_MEMORY_NAME: &[u8] = b"Local\\SCMonitorRoomInfo\0";

#[repr(C)]
pub struct RoomInfo {
    pub version: u32,                              // Structure version
    pub in_room: u32,                              // 1 if in a room, 0 if not
    pub last_update: u32,                          // Unix timestamp
    pub force_count: u32,                          // Number of forces (teams)
    pub room_id: [u8; MAX_ROOM_ID_LEN],            // Room/Session ID bytes
    pub room_name: [u8; MAX_ROOM_NAME_LEN],        // Room name
    pub map_name: [u8; MAX_MAP_NAME_LEN],          // Map name (Korean map names can be long)
    pub host_name: [u8; MAX_HOST_NAME_LEN],        // Host/creator name
    pub force_names: [[u8; MAX_FORCE_NAME_LEN]; MAX_FORCES], // Force/team names
}

// Global pointer to room info shared memory
static ROOM_INFO: AtomicPtr<RoomInfo> = AtomicPtr::new(std::ptr::null_mut());

// Latency/Ping shared memory structure
pub const MAX_PEERS: usize = 8;
pub const LATENCY_MEMORY_NAME: &[u8] = b"Local\\SCMonitorLatency\0";

// Drop Timer (dis-zero) shared memory structure
pub const DROP_TIMER_MEMORY_NAME: &[u8] = b"Local\\SCMonitorDropTimer\0";
pub const MAX_VERSION_LEN: usize = 32;

// Default offsets for StarCraft Remastered 1.23.10.13515
const DEFAULT_VERSION: &str = "1.23.10.13515";
const DEFAULT_VERSION_OFFSET_32: usize = 0xB2A220;
const DEFAULT_VERSION_OFFSET_64: usize = 0xDD4D18;
const DEFAULT_DROP_TIMER_OFFSET_32: usize = 0xDA41A0;
const DEFAULT_DROP_TIMER_OFFSET_64: usize = 0x109541C;

#[repr(C)]
pub struct DropTimerConfig {
    pub version: u32,                              // Structure version (1)
    pub enabled: u32,                              // 0 = disabled, 1 = enabled
    pub is_64bit: u32,                             // 0 = 32bit, 1 = 64bit (auto-detected)
    pub version_verified: u32,                     // 0 = not verified, 1 = verified
    pub drop_timer_offset: u32,                    // Current drop timer offset
    pub version_offset: u32,                       // Current version offset
    pub last_reset_time: u32,                      // Unix timestamp of last reset
    pub total_resets: u32,                         // Total number of resets performed
    pub current_timer_value: u32,                  // Current timer value (for monitoring)
    pub sc_version: [u8; MAX_VERSION_LEN],         // Expected SC version string
    pub detected_version: [u8; MAX_VERSION_LEN],   // Detected SC version string
    pub status: u32,                               // 0=waiting, 1=active, 2=version_mismatch, 3=error
}

// Global pointer to drop timer shared memory
static DROP_TIMER_CONFIG: AtomicPtr<DropTimerConfig> = AtomicPtr::new(std::ptr::null_mut());

// Drop timer reset interval (80ms like dis-zero)
const DROP_TIMER_RESET_INTERVAL_MS: u64 = 80;

// Last reset time tracking
static LAST_DROP_TIMER_RESET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PeerLatency {
    pub active: u32,                    // 1 if this slot is used
    pub ip_addr: u32,                   // IP address as u32
    pub port: u16,                      // Port number
    pub reserved: u16,                  // Padding
    pub last_send_time: u64,            // Last send timestamp (microseconds)
    pub last_recv_time: u64,            // Last recv timestamp (microseconds)
    pub current_rtt_us: u32,            // Current RTT in microseconds
    pub avg_rtt_us: u32,                // Moving average RTT in microseconds
    pub sample_count: u32,              // Number of samples taken
    pub padding: u32,                   // Alignment padding
}

#[repr(C)]
pub struct LatencyData {
    pub version: u32,                   // Structure version
    pub peer_count: u32,                // Number of active peers
    pub last_update: u32,               // Unix timestamp
    pub reserved: u32,                  // Padding
    pub peers: [PeerLatency; MAX_PEERS],
}

// Global pointer to latency shared memory
static LATENCY_DATA: AtomicPtr<LatencyData> = AtomicPtr::new(std::ptr::null_mut());

// High-precision timer frequency (set once at init)
static TIMER_FREQUENCY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct KickData {
    socket: SOCKET,
    dest_addr: SockAddrIn,
    last_kick_packet: Vec<u8>,
}

static KICK_DATA: Lazy<Mutex<KickData>> = Lazy::new(|| {
    Mutex::new(KickData {
        socket: 0,
        dest_addr: SockAddrIn {
            sin_family: 2,
            sin_port: 0,
            sin_addr: 0,
            sin_zero: [0; 8],
        },
        last_kick_packet: Vec::new(),
    })
});

static KICK_QUEUE: Lazy<Mutex<Vec<u16>>> = Lazy::new(|| Mutex::new(Vec::new()));

// Store peer info for auto-kick
struct PeerInfo {
    socket: SOCKET,
    addr: SockAddrIn,
}

static AUTO_KICK_TARGET: Lazy<Mutex<Option<PeerInfo>>> = Lazy::new(|| Mutex::new(None));

// Pending kick info for repeated kick (0s, 1s, 2s, 3s = 4 kicks total)
struct PendingKick {
    socket: SOCKET,
    addr: SockAddrIn,
    detected_time: std::time::Instant,
    kicks_sent: u32,  // How many kicks sent so far (0-4)
    last_kick_time: std::time::Instant,
}

static PENDING_KICK: Lazy<Mutex<Option<PendingKick>>> = Lazy::new(|| Mutex::new(None));

// File-based blacklist cache (unlimited entries, refreshed periodically)
static BLACKLIST_CACHE: Lazy<Mutex<Vec<Vec<u8>>>> = Lazy::new(|| Mutex::new(Vec::new()));

// Total kicks to send and interval
const TOTAL_KICKS: u32 = 4;
const KICK_INTERVAL_MS: u64 = 1000;

// ============================================================================
// My Session Info (for kick packet construction)
// ============================================================================

struct MySessionInfo {
    player_id: Vec<u8>,    // varint bytes (without 0x18 prefix)
    session_id: Vec<u8>,   // varint bytes (without 0x20 prefix)
    last_timestamp: u32,   // Latest game timestamp from packets
    captured: bool,
}

static MY_SESSION: Lazy<Mutex<MySessionInfo>> = Lazy::new(|| {
    Mutex::new(MySessionInfo {
        player_id: Vec::new(),
        session_id: Vec::new(),
        last_timestamp: 0,
        captured: false,
    })
});

// ============================================================================
// Logging (debug mode only)
// ============================================================================

#[cfg(debug_assertions)]
fn log_msg(msg: &str) {
    if let Ok(temp) = std::env::var("TEMP") {
        let log_path = format!("{}\\sc_monitor_dll.log", temp);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let _ = writeln!(file, "[{:.3}] {}", now.as_secs_f64(), msg);
        }
    }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
fn log_msg(_msg: &str) {}

#[cfg(debug_assertions)]
macro_rules! log {
    ($($arg:tt)*) => {
        log_msg(&format!($($arg)*))
    };
}

#[cfg(not(debug_assertions))]
macro_rules! log {
    ($($arg:tt)*) => { () };
}

// ============================================================================
// Utility Functions
// ============================================================================

// Format byte slice as hex string with spaces (e.g., "08 01 12 14")
fn format_hex(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ip_to_string(addr: u32) -> String {
    let bytes = addr.to_be_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

// Read varint from data at given position, return (value_bytes, bytes_consumed)
fn read_varint_bytes(data: &[u8], start: usize) -> Option<(Vec<u8>, usize)> {
    let mut bytes = Vec::new();
    let mut pos = start;

    while pos < data.len() {
        let b = data[pos];
        bytes.push(b);
        pos += 1;

        if b & 0x80 == 0 {
            // Last byte of varint
            return Some((bytes, pos - start));
        }

        // Prevent infinite loop on malformed data
        if bytes.len() > 10 {
            return None;
        }
    }

    None
}

// Update timestamp from outgoing packets
// Packet structure: [08, 01, 12, len, 00, 00, 00, 00, TS0, TS1, TS2, TS3, ...]
// Only works when len < 128 (single byte length field)
fn update_timestamp_from_packet(data: &[u8]) {
    // Must be at least 12 bytes and start with 08 01 12
    if data.len() < 12 || data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x12 {
        return;
    }

    // Only process packets with single-byte length field (len < 128)
    // This ensures correct offset for timestamp
    let len_byte = data[3];
    if len_byte >= 0x80 {
        // Length is a varint with multiple bytes, skip this packet
        return;
    }

    // Timestamp is at offset 8-11 (little-endian u32)
    let timestamp = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    // Only update if it's a reasonable timestamp (between 100k and 100M)
    // Real game timestamps are typically in the 900k-1M range initially
    // Always use latest valid timestamp (don't require increasing - resets per session)
    if timestamp >= 100_000 && timestamp <= 100_000_000 {
        let mut session = MY_SESSION.lock();
        session.last_timestamp = timestamp;
    }
}

// Extract My Player ID and Session ID from packet with pattern: 08 01 18 <player_id> 20 <session_id>
fn try_extract_my_session_info(data: &[u8]) {
    // Pattern: starts with 08 01 18 (message type 1, field 3 = player_id)
    if data.len() < 10 || data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x18 {
        return;
    }

    // Read player_id varint starting at offset 3
    let (player_id, player_id_len) = match read_varint_bytes(data, 3) {
        Some(v) => v,
        None => return,
    };

    // After player_id, expect 0x20 (field 4 = session_id)
    let session_id_tag_pos = 3 + player_id_len;
    if session_id_tag_pos >= data.len() || data[session_id_tag_pos] != 0x20 {
        return;
    }

    // Read session_id varint
    let (session_id, _) = match read_varint_bytes(data, session_id_tag_pos + 1) {
        Some(v) => v,
        None => return,
    };

    // Always update with latest session info (session ID changes per room)
    let mut session = MY_SESSION.lock();

    // Check if changed
    if session.captured && session.player_id == player_id && session.session_id == session_id {
        return; // Same as before, no need to log
    }

    let is_update = session.captured;
    session.player_id = player_id.clone();
    session.session_id = session_id.clone();
    session.captured = true;

    if is_update {
        log!("=== MY SESSION UPDATED ===");
    } else {
        log!("=== MY SESSION CAPTURED ===");
    }
    log!("  Player ID: {:02x?}", player_id);
    log!("  Session ID: {:02x?}", session_id);
}

fn is_kick_packet(data: &[u8]) -> bool {
    // BAN packet: 36 bytes, command type 01 00 at offset 16, command code 4e 01 at offset 20
    data.len() >= 22
        && data[0] == 0x08
        && data[1] == 0x01
        && data[2] == 0x12
        && data[3] == 0x12
        && data[16] == 0x01
        && data[17] == 0x00
        && data[20] == 0x4e
        && data[21] == 0x01
}

// Load blacklist targets from file written by Rust app (unlimited entries)
fn load_blacklist_from_file() -> Vec<Vec<u8>> {
    let local_app_data = match std::env::var("LOCALAPPDATA") {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let path = format!("{}\\r-launcher\\blacklist_targets.txt", local_app_data);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            content.lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| line.trim().as_bytes().to_vec())
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

// Refresh the in-memory blacklist cache from file
fn refresh_blacklist_cache() {
    let targets = load_blacklist_from_file();
    *BLACKLIST_CACHE.lock() = targets;
}

// Check if packet contains any auto-ban target battletag (file-based, unlimited)
// Returns the matched battletag if found, None otherwise
fn contains_ban_target(data: &[u8]) -> Option<String> {
    // Check if auto-ban is enabled via shared memory config
    let config_ptr = AUTOBAN_CONFIG.load(Ordering::SeqCst);
    if !config_ptr.is_null() {
        unsafe {
            if (*config_ptr).enabled == 0 {
                return None;
            }
        }
    }

    let cache = BLACKLIST_CACHE.lock();
    if cache.is_empty() {
        return None;
    }

    for target in cache.iter() {
        let target_len = target.len();
        if target_len == 0 {
            continue;
        }

        // Search for this target in the packet
        if data.len() >= target_len {
            for j in 0..=data.len().saturating_sub(target_len) {
                if data[j..j + target_len].eq_ignore_ascii_case(target) {
                    if let Ok(s) = std::str::from_utf8(target) {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }

    None
}

// Update auto-ban statistics in shared memory
fn update_autoban_stats(battletag: &str) {
    let config_ptr = AUTOBAN_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return;
    }

    unsafe {
        let config = &mut *config_ptr;
        config.total_kicks += 1;

        // Update last kick time (Unix timestamp)
        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            config.last_kick_time = duration.as_secs() as u32;
        }

        // Update last kicked battletag
        let bytes = battletag.as_bytes();
        let copy_len = bytes.len().min(MAX_BATTLETAG_LEN - 1);
        config.last_kicked_battletag[..copy_len].copy_from_slice(&bytes[..copy_len]);
        config.last_kicked_battletag[copy_len] = 0; // Null terminate
    }
}

// Extract battletag and nickname from User Info packet
// Returns (battletag, nickname)
fn extract_user_info_from_441_packet(data: &[u8]) -> Option<(String, String)> {
    // User Info packet identification by HEADER structure (not length):
    // - Header: 08 01 12 (offset 0-2)
    // - Varint length at offset 3:
    //   - Full User Info: a6 03 (varint = 422, 2 bytes) -> contains battletag + nickname
    //   - Packet total length varies (429-441 bytes observed)
    // - After varint: 00 00 00 00 [4B checksum] ...
    // - Marker at offset ~20-22: 3a 00 (for full user info)
    // - Battletag: offset ~26 from start, contains '#' character
    // - Nickname: offset ~200-230 from start

    if data.len() < 50 {
        return None;
    }

    // Check header pattern: 08 01 12
    if data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x12 {
        return None;
    }

    // Check for Full User Info packet by varint value
    // Varint a6 03 = (0x03 << 7) | (0xa6 & 0x7f) = 384 + 38 = 422
    // This is the payload length indicator for user info packets
    if data.len() < 5 {
        return None;
    }

    // Check if this is a Full User Info packet (varint = a6 03)
    let is_full_user_info = data[3] == 0xa6 && data[4] == 0x03;

    if !is_full_user_info {
        return None;
    }

    // Full User Info packet structure:
    // offset 0-2: 08 01 12 (header)
    // offset 3-4: a6 03 (varint length = 422)
    // offset 5-8: 00 00 00 00 (padding)
    // offset 9-12: checksum (4 bytes)
    // offset 13-20: various fields
    // offset 21-22: 3a 00 (user info marker)
    // offset 23-26: flags
    // offset 27+: battletag (UTF-8, null-terminated, contains #)
    // offset ~227+: nickname (ASCII, null-terminated)

    let data_start = 5; // After header (3) + varint (2)

    // Verify user info marker at offset 21-22 (data_start + 16-17)
    let marker_offset = data_start + 16;
    if data.len() < marker_offset + 2 {
        return None;
    }
    if data[marker_offset] != 0x3a || data[marker_offset + 1] != 0x00 {
        return None;
    }

    // Extract battletag - search for '#' character
    let battletag_search_start = data_start + 22; // offset ~27
    let battletag_search_end = std::cmp::min(data_start + 115, data.len());

    let mut hash_pos = None;
    for i in battletag_search_start..battletag_search_end {
        if data[i] == b'#' {
            hash_pos = Some(i);
            break;
        }
    }

    let hash_pos = hash_pos?;

    // Find start of battletag (go backwards to find null or padding)
    let mut battletag_start = battletag_search_start;
    for i in (battletag_search_start..hash_pos).rev() {
        if data[i] == 0 {
            battletag_start = i + 1;
            break;
        }
    }

    // Find end of battletag (null terminator after # and digits)
    let battletag_end = data[hash_pos..].iter()
        .position(|&b| b == 0)
        .map(|p| hash_pos + p)
        .unwrap_or(std::cmp::min(hash_pos + 10, data.len()));

    if battletag_start >= battletag_end {
        return None;
    }

    let battletag = std::str::from_utf8(&data[battletag_start..battletag_end])
        .ok()?
        .to_string();

    // Validate battletag (must contain #)
    if !battletag.contains('#') {
        return None;
    }

    // Find nickname in range 200-340 (search for first non-zero ASCII sequence)
    let nickname_start = std::cmp::min(200, data.len());
    let nickname_end = std::cmp::min(340, data.len());
    let nickname = if nickname_start < nickname_end {
        find_nickname_in_range(&data[nickname_start..nickname_end]).unwrap_or_default()
    } else {
        String::new()
    };

    Some((battletag, nickname))
}

// Find nickname string in a range of bytes (first non-zero ASCII sequence)
fn find_nickname_in_range(data: &[u8]) -> Option<String> {
    // Find first non-zero byte
    let start = data.iter().position(|&b| b != 0)?;
    let end = data[start..].iter()
        .position(|&b| b == 0)
        .map(|p| start + p)
        .unwrap_or(data.len());

    std::str::from_utf8(&data[start..end]).ok().map(|s| s.to_string())
}

// Helper to extract session/room ID from packet tags
fn extract_session_id_from_tags(data: &[u8]) -> Option<Vec<u8>> {
    // Search for TAG 0x20 (Field 4, Wire Type 0)
    // To be safer, look for 0x18 (Field 3) followed by its varint, then 0x20
    for i in 0..data.len().saturating_sub(4) {
        if data[i] == 0x18 {
            if let Some((_, len)) = read_varint_bytes(data, i + 1) {
                let next_pos = i + 1 + len;
                if next_pos < data.len() && data[next_pos] == 0x20 {
                    if let Some((sid_bytes, _)) = read_varint_bytes(data, next_pos + 1) {
                        return Some(sid_bytes);
                    }
                }
            }
        }
    }
    None
}

// Extract room info from room join packet (WSARECVFROM or WSASENDTO)
// Returns (room_name, map_name, host_name, room_id)
fn extract_room_info_from_packet(data: &[u8]) -> Option<(String, String, String, Vec<u8>)> {
    // Room info packet structure (Full Room Info, Type 08):
    // - Header: 08 01 12 (offset 0-2)
    // - Length: varint at offset 3 (1 byte if < 0x80, 2 bytes if >= 0x80)
    // - Padding: 00 00 00 00
    // - Checksum: 4 bytes
    // - Sequence: 4 bytes (seq1 2 bytes, seq2 2 bytes)
    // - Message Type: 00 08 (Room Info command) - position depends on varint length!
    //   - 1-byte varint: offset 14-15
    //   - 2-byte varint: offset 15-16
    // - Room name: after header, null-terminated
    //
    // Packet lengths observed: 140-236 bytes

    if data.len() < 100 {
        return None;
    }

    // Check header pattern: 08 01 12
    if data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x12 {
        return None;
    }

    // Determine varint length
    let varint_len = if data[3] >= 0x80 { 2 } else { 1 };
    let data_start = 3 + varint_len;

    // Check Room Info command type: 00 08
    // Position: data_start + 4 (padding) + 4 (checksum) + 2 (seq1) + 2 (seq2) = data_start + 12
    // Command type is at data_start + 12 and data_start + 13
    let cmd_offset = data_start + 12;
    if data.len() < cmd_offset + 2 {
        return None;
    }
    if data[cmd_offset] != 0x00 || data[cmd_offset + 1] != 0x08 {
        return None; // Not a Room Info packet (Type 08)
    }

    // Room name starts at offset 36 from data_start
    let room_name_start = data_start + 36;
    if room_name_start >= data.len() {
        return None;
    }

    // Find room name (null-terminated ASCII)
    let room_name_end = data[room_name_start..].iter()
        .position(|&b| b == 0)
        .map(|p| room_name_start + p)
        .unwrap_or(room_name_start);

    if room_name_end <= room_name_start {
        return None;
    }

    let room_name = std::str::from_utf8(&data[room_name_start..room_name_end])
        .ok()?
        .to_string();

    // After room name null, find the CSV data section
    let csv_start = room_name_end + 1;
    if csv_start >= data.len() {
        return Some((room_name, String::new(), String::new(), Vec::new()));
    }

    // Find first \r (0x0d) - this ends the CSV section containing host name
    let first_cr = data[csv_start..].iter()
        .position(|&b| b == 0x0d)
        .map(|p| csv_start + p);

    let host_name = if let Some(cr_pos) = first_cr {
        // CSV section: ",,28,6,,f,,4,18,1,,hostname"
        // Host name is the last field (after the last comma before \r)
        let csv_section = &data[csv_start..cr_pos];
        if let Ok(csv_str) = std::str::from_utf8(csv_section) {
            // Split by comma and get last non-empty field
            csv_str.rsplit(',')
                .find(|s| !s.is_empty())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Find map name: after first \r, until next \r\r or double \r
    let map_name = if let Some(cr_pos) = first_cr {
        let map_start = cr_pos + 1;
        if map_start < data.len() {
            // Find end of map name (look for \r\r pattern or null)
            let map_end = data[map_start..].windows(2)
                .position(|w| (w[0] == 0x0d && w[1] == 0x0d) || w[0] == 0)
                .map(|p| map_start + p)
                .unwrap_or_else(|| {
                    // Fallback: find single \r or null
                    data[map_start..].iter()
                        .position(|&b| b == 0x0d || b == 0)
                        .map(|p| map_start + p)
                        .unwrap_or(data.len())
                });

            if map_end > map_start {
                // Filter out StarCraft color codes (0x01-0x1F control characters)
                // but keep valid UTF-8 sequences
                let filtered: Vec<u8> = data[map_start..map_end]
                    .iter()
                    .copied()
                    .filter(|&b| b >= 0x20 || b == 0x09) // Keep printable chars and tab
                    .collect();

                String::from_utf8(filtered)
                    .unwrap_or_else(|_| {
                        // If still invalid UTF-8, use lossy conversion
                        String::from_utf8_lossy(&data[map_start..map_end])
                            .chars()
                            .filter(|c| !c.is_control() || *c == '\t')
                            .collect()
                    })
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Extract Session ID as Room ID from tags
    let room_id = extract_session_id_from_tags(data).unwrap_or_default();

    Some((room_name, map_name, host_name, room_id))
}

// Extract force (team) info from force packet (WSARECVFROM)
// Returns vector of force names
fn extract_force_info_from_packet(data: &[u8]) -> Option<Vec<String>> {
    // Force info packet structure:
    // - Header: 08 01 12 (offset 0-2)
    // - Length: varint at offset 3 (1 byte if < 0x80, 2 bytes if >= 0x80)
    // - After length field: data starts
    // - Force info identifier: 00 00 00 at offset 8 from data start
    // - Force names are null-terminated strings starting with '[' character
    //
    // Force packets are typically 100-200 bytes
    // User Info packets are 400+ bytes and should NOT be detected as force
    //
    // Example force names: "[Nature Perfect.ver]", "[By.Bread_183]"

    // Force packets are typically 100-200 bytes, User Info is 400+
    if data.len() < 100 || data.len() > 250 {
        return None;
    }

    // Check header pattern: 08 01 12
    if data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x12 {
        return None;
    }

    // Determine varint length (offset 3)
    let varint_len = if data[3] >= 0x80 { 2 } else { 1 };
    let data_start = 3 + varint_len;

    // Check if this is a force info packet (not room info)
    // Force info has 00 00 00 at offset 8 from data_start
    let id_offset = data_start + 8;
    if data.len() < id_offset + 3 {
        return None;
    }
    if data[id_offset] != 0x00 || data[id_offset + 1] != 0x00 || data[id_offset + 2] != 0x00 {
        return None; // This is room info or other packet, not force info
    }

    // Additional check: Force packets have 01 00 00 00 at offset 15-18 from data_start
    // User Info packets have different structure
    let check_offset = data_start + 15;
    if data.len() < check_offset + 4 {
        return None;
    }
    if data[check_offset] != 0x01 || data[check_offset + 1] != 0x00 ||
       data[check_offset + 2] != 0x00 || data[check_offset + 3] != 0x00 {
        return None;
    }

    let mut forces = Vec::new();
    let mut pos = data_start + 17; // Start scanning from appropriate offset

    while pos < data.len() && forces.len() < MAX_FORCES {
        // Look for null-terminated strings
        // Skip non-printable bytes until we find a printable character
        while pos < data.len() && (data[pos] < 0x20 || data[pos] == 0x00) {
            pos += 1;
        }

        if pos >= data.len() {
            break;
        }

        // Find end of string (null terminator)
        let start = pos;
        while pos < data.len() && data[pos] != 0x00 {
            pos += 1;
        }

        if pos > start {
            // Filter out control characters and try to parse as UTF-8
            let filtered: Vec<u8> = data[start..pos]
                .iter()
                .copied()
                .filter(|&b| b >= 0x20 || b == 0x09)
                .collect();

            if !filtered.is_empty() {
                if let Ok(s) = String::from_utf8(filtered.clone()) {
                    // Only add if it looks like a force name (not garbage)
                    // Force names typically don't start with common prefixes like "feedda7a"
                    if !s.starts_with("feedda") && !s.is_empty() && s.len() <= MAX_FORCE_NAME_LEN {
                        forces.push(s);
                        log!("FORCE DETECTED: '{}'", forces.last().unwrap());
                    }
                }
            }
        }

        pos += 1; // Skip null terminator
    }

    if forces.is_empty() {
        None
    } else {
        Some(forces)
    }
}

// Update force info in shared memory
fn update_force_info(forces: &[String]) {
    let room_ptr = ROOM_INFO.load(Ordering::SeqCst);
    if room_ptr.is_null() {
        return;
    }

    unsafe {
        let room = &mut *room_ptr;

        // Clear existing force names
        for i in 0..MAX_FORCES {
            room.force_names[i] = [0; MAX_FORCE_NAME_LEN];
        }

        // Copy new force names
        room.force_count = forces.len().min(MAX_FORCES) as u32;
        for (i, force) in forces.iter().take(MAX_FORCES).enumerate() {
            let bytes = force.as_bytes();
            let copy_len = bytes.len().min(MAX_FORCE_NAME_LEN - 1);
            room.force_names[i][..copy_len].copy_from_slice(&bytes[..copy_len]);
        }

        // Update timestamp
        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            room.last_update = duration.as_secs() as u32;
        }
    }

    log!("FORCE INFO UPDATED: {} forces", forces.len());
}

// Clear all room users (called when joining a new room)
fn clear_room_users() {
    let room_ptr = ROOM_USERS.load(Ordering::SeqCst);
    if room_ptr.is_null() {
        return;
    }

    unsafe {
        let room = &mut *room_ptr;

        // Clear all user slots
        for i in 0..MAX_ROOM_USERS {
            room.users[i].active = 0;
            room.users[i].nickname = [0; MAX_NICKNAME_LEN];
            room.users[i].battletag = [0; MAX_BATTLETAG_LEN];
            room.users[i].ip_address = [0; MAX_IP_LEN];
            room.users[i].port = 0;
            room.users[i].player_id = 0;
            room.users[i].flags = 0;
        }

        room.user_count = 0;

        // Update timestamp
        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            room.last_update = duration.as_secs() as u32;
        }
    }

    log!("ROOM USERS CLEARED (new room joined)");
}

// Clear all latency peers (called when joining a new room to prevent stale peer data)
fn clear_latency_peers() {
    let latency_ptr = LATENCY_DATA.load(Ordering::SeqCst);
    if latency_ptr.is_null() {
        return;
    }

    unsafe {
        let data = &mut *latency_ptr;
        for i in 0..MAX_PEERS {
            data.peers[i].active = 0;
            data.peers[i].ip_addr = 0;
            data.peers[i].port = 0;
            data.peers[i].last_send_time = 0;
            data.peers[i].last_recv_time = 0;
            data.peers[i].current_rtt_us = 0;
            data.peers[i].avg_rtt_us = 0;
            data.peers[i].sample_count = 0;
        }
        data.peer_count = 0;
    }

    log!("LATENCY PEERS CLEARED (new room joined)");
}

// Update room info (room name, map name, host name, and room ID) in shared memory
fn update_room_info(room_name: &str, map_name: &str, host_name: &str, room_id: &[u8]) {
    let room_ptr = ROOM_INFO.load(Ordering::SeqCst);
    if room_ptr.is_null() {
        return;
    }

    unsafe {
        let room = &mut *room_ptr;

        room.in_room = 1;

        // Copy room ID
        room.room_id = [0; MAX_ROOM_ID_LEN];
        let id_len = room_id.len().min(MAX_ROOM_ID_LEN);
        room.room_id[..id_len].copy_from_slice(&room_id[..id_len]);

        // Copy room name
        room.room_name = [0; MAX_ROOM_NAME_LEN];
        let name_bytes = room_name.as_bytes();
        let copy_len = name_bytes.len().min(MAX_ROOM_NAME_LEN - 1);
        room.room_name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

        // Copy map name
        room.map_name = [0; MAX_MAP_NAME_LEN];
        let map_bytes = map_name.as_bytes();
        let map_len = map_bytes.len().min(MAX_MAP_NAME_LEN - 1);
        room.map_name[..map_len].copy_from_slice(&map_bytes[..map_len]);

        // Copy host name
        room.host_name = [0; MAX_HOST_NAME_LEN];
        let host_bytes = host_name.as_bytes();
        let host_len = host_bytes.len().min(MAX_HOST_NAME_LEN - 1);
        room.host_name[..host_len].copy_from_slice(&host_bytes[..host_len]);

        // Update timestamp
        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            room.last_update = duration.as_secs() as u32;
        }
    }
}

// Extract nickname from PNM: pattern in packet data
// Pattern: "PNM:" followed by nickname, ends with null byte or '.'
fn extract_nickname_from_pnm(data: &[u8]) -> Option<String> {
    // Look for "PNM:" pattern (50 4e 4d 3a)
    let pnm_pattern = b"PNM:";

    for i in 0..data.len().saturating_sub(4) {
        if &data[i..i+4] == pnm_pattern {
            // Found PNM:, extract nickname until null or '.' or end
            let start = i + 4;
            let mut end = start;

            while end < data.len() {
                let b = data[end];
                // Stop at null, '.', or non-printable characters
                if b == 0 || b == b'.' {
                    break;
                }
                end += 1;
            }

            if end > start {
                if let Ok(nickname) = std::str::from_utf8(&data[start..end]) {
                    // Filter out invalid nicknames (too short or contains invalid chars)
                    if nickname.len() >= 2 && !nickname.contains("_blizzard") {
                        return Some(nickname.to_string());
                    }
                }
            }
        }
    }
    None
}

// Add room user with battletag, nickname, IP, and port
fn add_room_user_with_info(battletag: &str, nickname: &str, ip: &str, port: u16) {
    let room_ptr = ROOM_USERS.load(Ordering::SeqCst);
    if room_ptr.is_null() {
        return;
    }

    unsafe {
        let room = &mut *room_ptr;

        // Check if user already exists (by battletag)
        for i in 0..MAX_ROOM_USERS {
            if room.users[i].active == 1 {
                let existing_len = room.users[i].battletag.iter()
                    .position(|&b| b == 0)
                    .unwrap_or(MAX_BATTLETAG_LEN);
                if let Ok(existing) = std::str::from_utf8(&room.users[i].battletag[..existing_len]) {
                    if existing == battletag {
                        return; // Already exists
                    }
                }
            }
        }

        // Use provided nickname, or extract from battletag as fallback
        let final_nickname = if !nickname.is_empty() {
            nickname.to_string()
        } else if let Some(hash_pos) = battletag.find('#') {
            battletag[..hash_pos].to_string()
        } else {
            String::new()
        };

        // Find empty slot
        for i in 0..MAX_ROOM_USERS {
            if room.users[i].active == 0 {
                room.users[i].active = 1;
                room.users[i].player_id = i as u32;

                // Copy battletag
                let bytes = battletag.as_bytes();
                let copy_len = bytes.len().min(MAX_BATTLETAG_LEN - 1);
                room.users[i].battletag[..copy_len].copy_from_slice(&bytes[..copy_len]);
                room.users[i].battletag[copy_len] = 0;

                let nick_bytes = final_nickname.as_bytes();
                let nick_len = nick_bytes.len().min(MAX_NICKNAME_LEN - 1);
                room.users[i].nickname[..nick_len].copy_from_slice(&nick_bytes[..nick_len]);
                room.users[i].nickname[nick_len] = 0;

                // Copy IP address
                let ip_bytes = ip.as_bytes();
                let ip_len = ip_bytes.len().min(MAX_IP_LEN - 1);
                room.users[i].ip_address[..ip_len].copy_from_slice(&ip_bytes[..ip_len]);
                room.users[i].ip_address[ip_len] = 0;

                // Store port
                room.users[i].port = port;
                room.users[i].flags = 0;

                room.user_count += 1;

                // Update timestamp
                if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    room.last_update = duration.as_secs() as u32;
                }

                log!("ROOM USER ADDED: {} (nick={}, ip={}:{}, slot {})",
                    battletag, final_nickname, ip, port, i);
                return;
            }
        }

        log!("ROOM FULL: Cannot add {}", battletag);
    }
}

// Check if packet is a disconnect notification
// Pattern: 42 bytes, offset 16-17 = 00 0b (packet type = DISCONNECT)
fn is_disconnect_packet(data: &[u8]) -> bool {
    // Must be exactly 42 bytes with the disconnect signature
    if data.len() < 18 {
        return false;
    }

    // Check header: 08 01 12 18 (message type 1, field 2 with length 24)
    if data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x12 || data[3] != 0x18 {
        return false;
    }

    // Check disconnect type at offset 16-17: 00 0b
    data[16] == 0x00 && data[17] == 0x0b
}

// Remove room user by IP address
fn remove_room_user_by_ip(ip: u32) {
    let room_ptr = ROOM_USERS.load(Ordering::SeqCst);
    if room_ptr.is_null() {
        return;
    }

    let ip_str = ip_to_string(ip);

    unsafe {
        let room = &mut *room_ptr;

        for i in 0..MAX_ROOM_USERS {
            if room.users[i].active == 1 {
                // Compare IP address
                let user_ip_len = room.users[i].ip_address.iter()
                    .position(|&b| b == 0)
                    .unwrap_or(MAX_IP_LEN);

                if let Ok(user_ip) = std::str::from_utf8(&room.users[i].ip_address[..user_ip_len]) {
                    if user_ip == ip_str {
                        // Get battletag for logging
                        let battletag_len = room.users[i].battletag.iter()
                            .position(|&b| b == 0)
                            .unwrap_or(MAX_BATTLETAG_LEN);
                        let battletag = std::str::from_utf8(&room.users[i].battletag[..battletag_len])
                            .unwrap_or("unknown");

                        log!("ROOM USER REMOVED: {} (ip={}, slot {})", battletag, ip_str, i);

                        // Clear the slot
                        room.users[i].active = 0;
                        room.users[i].player_id = 0;
                        room.users[i].nickname = [0; MAX_NICKNAME_LEN];
                        room.users[i].battletag = [0; MAX_BATTLETAG_LEN];
                        room.users[i].ip_address = [0; MAX_IP_LEN];
                        room.users[i].port = 0;
                        room.users[i].flags = 0;

                        if room.user_count > 0 {
                            room.user_count -= 1;
                        }

                        // Update timestamp
                        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                            room.last_update = duration.as_secs() as u32;
                        }

                        break;
                    }
                }
            }
        }
    }
}

// Remove latency data by IP address
fn remove_latency_by_ip(ip: u32) {
    let latency_ptr = LATENCY_DATA.load(Ordering::SeqCst);
    if latency_ptr.is_null() {
        return;
    }

    unsafe {
        let data = &mut *latency_ptr;

        for i in 0..MAX_PEERS {
            if data.peers[i].active == 1 && data.peers[i].ip_addr == ip {
                log!("LATENCY PEER REMOVED: ip={}", ip_to_string(ip));

                data.peers[i].active = 0;
                data.peers[i].ip_addr = 0;
                data.peers[i].port = 0;
                data.peers[i].last_send_time = 0;
                data.peers[i].last_recv_time = 0;
                data.peers[i].current_rtt_us = 0;
                data.peers[i].avg_rtt_us = 0;
                data.peers[i].sample_count = 0;

                if data.peer_count > 0 {
                    data.peer_count -= 1;
                }

                break;
            }
        }
    }
}

// Extract room name from WSASENDTO packet
// Room name pattern: packet starts with 08 01 12 XX where XX >= 0x50, room name at offset 40
fn extract_room_name_from_packet(data: &[u8]) -> Option<String> {
    // Need packet with: 08 01 12 [length >= 0x50] and at least 50 bytes
    if data.len() < 50 {
        return None;
    }

    // Check signature: 08, 01, 12, and length field >= 0x50 (80 bytes)
    if data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x12 {
        return None;
    }

    // Length field should indicate a large packet (room creation packets are > 100 bytes)
    let length = data[3] as usize;
    if length < 0x50 {
        return None;
    }

    // Room name starts around offset 40
    let room_name_start = 40;
    if room_name_start >= data.len() {
        return None;
    }

    // Find null terminator for room name
    let mut room_name_end = room_name_start;
    while room_name_end < data.len() && data[room_name_end] != 0 {
        // Only allow printable ASCII characters
        if data[room_name_end] < 0x20 || data[room_name_end] > 0x7E {
            break;
        }
        room_name_end += 1;
    }

    // Room name should be at least 1 character
    if room_name_end <= room_name_start {
        return None;
    }

    match std::str::from_utf8(&data[room_name_start..room_name_end]) {
        Ok(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

// Extract map name from WSASENDTO packet - search for ".scm" pattern
fn extract_map_name_from_packet(data: &[u8]) -> Option<String> {
    // Search for ".scm" pattern
    for i in 0..data.len().saturating_sub(4) {
        if data[i] == b'.' && data[i+1] == b's' && data[i+2] == b'c' && data[i+3] == b'm' {
            // Found .scm, now go back to find the start (look for '(' or printable chars)
            let mut start = i;
            while start > 0 && data[start - 1] >= 0x20 && data[start - 1] < 0x7F {
                start -= 1;
            }

            if let Ok(s) = std::str::from_utf8(&data[start..i+4]) {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
            break;
        }
    }
    None
}

// Update room name in shared memory
fn update_room_name(room_name: &str) {
    let room_ptr = ROOM_INFO.load(Ordering::SeqCst);
    if room_ptr.is_null() {
        return;
    }

    unsafe {
        let room = &mut *room_ptr;

        room.in_room = 1;

        // Copy room name
        room.room_name = [0; MAX_ROOM_NAME_LEN];
        let name_bytes = room_name.as_bytes();
        let copy_len = name_bytes.len().min(MAX_ROOM_NAME_LEN - 1);
        room.room_name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

        // Update timestamp
        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            room.last_update = duration.as_secs() as u32;
        }
    }
}

// Update map name in shared memory
fn update_map_name(map_name: &str) {
    let room_ptr = ROOM_INFO.load(Ordering::SeqCst);
    if room_ptr.is_null() {
        return;
    }

    unsafe {
        let room = &mut *room_ptr;

        room.in_room = 1;

        // Copy map name
        room.map_name = [0; MAX_MAP_NAME_LEN];
        let map_bytes = map_name.as_bytes();
        let map_len = map_bytes.len().min(MAX_MAP_NAME_LEN - 1);
        room.map_name[..map_len].copy_from_slice(&map_bytes[..map_len]);

        // Update timestamp
        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            room.last_update = duration.as_secs() as u32;
        }
    }
}

// Get current time in microseconds using high-precision timer
fn get_time_us() -> u64 {
    let freq = TIMER_FREQUENCY.load(Ordering::Relaxed);
    if freq == 0 {
        return 0;
    }

    unsafe {
        let mut counter: i64 = 0;
        if QueryPerformanceCounter(&mut counter).is_ok() {
            // Convert to microseconds
            ((counter as u64) * 1_000_000) / freq
        } else {
            0
        }
    }
}

// Initialize high-precision timer
fn init_timer() {
    unsafe {
        let mut freq: i64 = 0;
        if QueryPerformanceFrequency(&mut freq).is_ok() {
            TIMER_FREQUENCY.store(freq as u64, Ordering::Relaxed);
            log!("Timer initialized: frequency = {} Hz", freq);
        }
    }
}

// ============================================================================
// Drop Timer (dis-zero) Functions
// ============================================================================

// Get StarCraft module base address
fn get_sc_base_address() -> Option<usize> {
    unsafe {
        // Get the main module handle (StarCraft.exe)
        match GetModuleHandleA(PCSTR(std::ptr::null())) {
            Ok(h) => Some(h.0 as usize),
            Err(_) => None,
        }
    }
}

// Detect if running in 64-bit mode
fn is_64bit_process() -> bool {
    std::mem::size_of::<usize>() == 8
}

// Get appropriate offsets based on architecture
fn get_drop_timer_offset(is_64bit: bool) -> usize {
    if is_64bit {
        DEFAULT_DROP_TIMER_OFFSET_64
    } else {
        DEFAULT_DROP_TIMER_OFFSET_32
    }
}

fn get_version_offset(is_64bit: bool) -> usize {
    if is_64bit {
        DEFAULT_VERSION_OFFSET_64
    } else {
        DEFAULT_VERSION_OFFSET_32
    }
}

// Verify StarCraft version by reading from memory
fn verify_sc_version(base_addr: usize, is_64bit: bool) -> Option<String> {
    let version_offset = get_version_offset(is_64bit);
    let version_addr = base_addr + version_offset;
    let expected_len = DEFAULT_VERSION.len();

    unsafe {
        let version_ptr = version_addr as *const u8;

        // Read version string from memory
        let mut version_bytes = Vec::with_capacity(expected_len);
        for i in 0..expected_len {
            let byte = *version_ptr.add(i);
            if byte == 0 {
                break;
            }
            version_bytes.push(byte);
        }

        match std::str::from_utf8(&version_bytes) {
            Ok(s) => Some(s.to_string()),
            Err(_) => None,
        }
    }
}

// Reset drop timer to 0 (with safe memory access)
fn reset_drop_timer() {
    let config_ptr = DROP_TIMER_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return;
    }

    unsafe {
        let config = &mut *config_ptr;

        // Check if enabled
        if config.enabled == 0 {
            return;
        }

        // Check reset interval (80ms)
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let last_reset = LAST_DROP_TIMER_RESET.load(Ordering::Relaxed);
        if now_ms.saturating_sub(last_reset) < DROP_TIMER_RESET_INTERVAL_MS {
            return;
        }

        // Update last reset check time first to prevent rapid retries on error
        LAST_DROP_TIMER_RESET.store(now_ms, Ordering::Relaxed);

        // Get base address
        let base_addr = match get_sc_base_address() {
            Some(addr) => addr,
            None => {
                config.status = 3; // error
                return;
            }
        };

        // Sanity check base address
        if base_addr == 0 || base_addr < 0x10000 {
            config.status = 3; // error
            return;
        }

        // Detect architecture
        let is_64bit = is_64bit_process();
        config.is_64bit = if is_64bit { 1 } else { 0 };

        // Get drop timer offset
        let drop_timer_offset = get_drop_timer_offset(is_64bit);
        config.drop_timer_offset = drop_timer_offset as u32;
        config.version_offset = get_version_offset(is_64bit) as u32;

        // Calculate drop timer address
        let timer_addr = base_addr.wrapping_add(drop_timer_offset);

        // Safety check: ensure address is reasonable
        // For 32-bit processes, valid user-mode addresses are typically below 0x7FFFFFFF
        if timer_addr < 0x10000 || timer_addr > 0x7FFFFFFF {
            config.status = 3; // error
            return;
        }

        // Try to read/write with exception handling using IsBadReadPtr/IsBadWritePtr
        let timer_ptr = timer_addr as *mut u32;

        // Use Windows API to check if memory is accessible
        if is_bad_read_ptr(timer_ptr as *const u8, 4) {
            config.status = 0; // waiting (memory not accessible yet)
            return;
        }

        // Read current timer value
        let current_value = std::ptr::read_volatile(timer_ptr);
        config.current_timer_value = current_value;
        config.status = 1; // active
        config.version_verified = 1;

        // Reset timer if it's >= 1
        if current_value >= 1 {
            if !is_bad_write_ptr(timer_ptr as *mut u8, 4) {
                std::ptr::write_volatile(timer_ptr, 0);
                config.total_resets += 1;

                // Update last reset time
                if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    config.last_reset_time = duration.as_secs() as u32;
                }

                // Only log occasionally to reduce spam
                if config.total_resets <= 3 || config.total_resets % 100 == 0 {
                    log!("[DROP-TIMER] Timer reset: {} -> 0 (total: {})", current_value, config.total_resets);
                }
            }
        }
    }
}

// Safe memory check functions
fn is_bad_read_ptr(ptr: *const u8, size: usize) -> bool {
    use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, PAGE_NOACCESS, PAGE_GUARD};

    unsafe {
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        let result = VirtualQuery(
            Some(ptr as *const std::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        );

        if result == 0 {
            return true; // Cannot query, assume bad
        }

        // Check if memory is accessible for reading
        let protect = mbi.Protect;
        if protect == PAGE_NOACCESS || protect == PAGE_GUARD {
            return true;
        }

        // Check if the full range is within this region
        let region_end = (mbi.BaseAddress as usize) + mbi.RegionSize;
        let ptr_end = (ptr as usize) + size;

        ptr_end > region_end
    }
}

fn is_bad_write_ptr(ptr: *mut u8, size: usize) -> bool {
    use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, PAGE_NOACCESS, PAGE_GUARD, PAGE_READONLY, PAGE_EXECUTE, PAGE_EXECUTE_READ};

    unsafe {
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        let result = VirtualQuery(
            Some(ptr as *const std::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        );

        if result == 0 {
            return true;
        }

        let protect = mbi.Protect;
        if protect == PAGE_NOACCESS || protect == PAGE_GUARD ||
           protect == PAGE_READONLY || protect == PAGE_EXECUTE || protect == PAGE_EXECUTE_READ {
            return true;
        }

        let region_end = (mbi.BaseAddress as usize) + mbi.RegionSize;
        let ptr_end = (ptr as usize) + size;

        ptr_end > region_end
    }
}

// Initialize drop timer with default version info
fn init_drop_timer_config(config: &mut DropTimerConfig) {
    config.version = 1;
    config.enabled = 1; // Enabled by default (dis-zero active)
    config.is_64bit = if is_64bit_process() { 1 } else { 0 };
    config.version_verified = 0;
    config.drop_timer_offset = 0;
    config.version_offset = 0;
    config.last_reset_time = 0;
    config.total_resets = 0;
    config.current_timer_value = 0;
    config.status = 0; // waiting

    // Set expected version
    let version_bytes = DEFAULT_VERSION.as_bytes();
    let copy_len = version_bytes.len().min(MAX_VERSION_LEN - 1);
    config.sc_version[..copy_len].copy_from_slice(&version_bytes[..copy_len]);
    config.sc_version[copy_len] = 0;

    // Clear detected version
    config.detected_version = [0; MAX_VERSION_LEN];
}

// Record send time for a peer
fn record_send_time(ip: u32, port: u16) {
    let latency_ptr = LATENCY_DATA.load(Ordering::SeqCst);
    if latency_ptr.is_null() {
        log!("[LATENCY] record_send_time: LATENCY_DATA is null!");
        return;
    }

    let now = get_time_us();
    if now == 0 {
        log!("[LATENCY] record_send_time: get_time_us returned 0 (timer not initialized?)");
        return;
    }

    unsafe {
        let data = &mut *latency_ptr;

        // Find existing peer or empty slot
        let mut found_idx: Option<usize> = None;
        let mut empty_idx: Option<usize> = None;

        for i in 0..MAX_PEERS {
            if data.peers[i].active == 1 && data.peers[i].ip_addr == ip {
                found_idx = Some(i);
                break;
            }
            if data.peers[i].active == 0 && empty_idx.is_none() {
                empty_idx = Some(i);
            }
        }

        let idx = if let Some(i) = found_idx {
            i
        } else if let Some(i) = empty_idx {
            // New peer
            data.peers[i].active = 1;
            data.peers[i].ip_addr = ip;
            data.peers[i].port = port;
            data.peers[i].current_rtt_us = 0;
            data.peers[i].avg_rtt_us = 0;
            data.peers[i].sample_count = 0;
            data.peer_count += 1;
            log!("[LATENCY] New peer added (send): ip={} port={}", ip_to_string(ip), port);
            i
        } else {
            log!("[LATENCY] No space for new peer!");
            return; // No space
        };

        data.peers[idx].last_send_time = now;
    }
}

// Record recv time for a peer and calculate RTT
fn record_recv_time(ip: u32, port: u16) {
    let latency_ptr = LATENCY_DATA.load(Ordering::SeqCst);
    if latency_ptr.is_null() {
        log!("[LATENCY] record_recv_time: LATENCY_DATA is null!");
        return;
    }

    let now = get_time_us();
    if now == 0 {
        log!("[LATENCY] record_recv_time: get_time_us returned 0!");
        return;
    }

    unsafe {
        let data = &mut *latency_ptr;

        // Find peer
        let mut found = false;
        for i in 0..MAX_PEERS {
            if data.peers[i].active == 1 && data.peers[i].ip_addr == ip {
                found = true;
                data.peers[i].last_recv_time = now;

                // Calculate RTT if we have a send time
                let send_time = data.peers[i].last_send_time;
                if send_time > 0 && now > send_time {
                    let rtt = (now - send_time) as u32;

                    // Only count reasonable RTT values (< 10 seconds)
                    if rtt < 10_000_000 {
                        data.peers[i].current_rtt_us = rtt;

                        // Update moving average (exponential smoothing: 0.7 old + 0.3 new)
                        if data.peers[i].sample_count == 0 {
                            data.peers[i].avg_rtt_us = rtt;
                        } else {
                            let old_avg = data.peers[i].avg_rtt_us as u64;
                            let new_avg = (old_avg * 7 + rtt as u64 * 3) / 10;
                            data.peers[i].avg_rtt_us = new_avg as u32;
                        }

                        data.peers[i].sample_count += 1;

                        // Log RTT update (only every 100 samples to reduce spam)
                        if data.peers[i].sample_count % 100 == 1 {
                            log!("[LATENCY] RTT updated: ip={} rtt={}us avg={}us samples={}",
                                ip_to_string(ip), rtt, data.peers[i].avg_rtt_us, data.peers[i].sample_count);
                        }

                        // Update timestamp
                        if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                            data.last_update = duration.as_secs() as u32;
                        }
                    }
                }
                break;
            }
        }

        // If peer not found, add it
        if !found {
            for i in 0..MAX_PEERS {
                if data.peers[i].active == 0 {
                    data.peers[i].active = 1;
                    data.peers[i].ip_addr = ip;
                    data.peers[i].port = port;
                    data.peers[i].last_recv_time = now;
                    data.peers[i].current_rtt_us = 0;
                    data.peers[i].avg_rtt_us = 0;
                    data.peers[i].sample_count = 0;
                    data.peer_count += 1;
                    log!("[LATENCY] New peer added: ip={} port={}", ip_to_string(ip), port);
                    break;
                }
            }
        }
    }
}

// Build and send BAN packet to peer using dynamic session info
unsafe fn send_ban_packet(socket: SOCKET, addr: &SockAddrIn) {
    // Check if we have captured session info
    let (player_id, session_id, timestamp) = {
        let session = MY_SESSION.lock();
        if !session.captured {
            log!("Cannot send BAN: Session info not captured yet");
            return;
        }
        // Use captured timestamp, or fallback to default if not captured yet
        let ts = if session.last_timestamp > 0 {
            session.last_timestamp
        } else {
            980640 // fallback
        };
        (session.player_id.clone(), session.session_id.clone(), ts)
    };

    // Build BAN packet dynamically
    // Correct protobuf structure:
    // 08 01        - field 1: message type = 1
    // 12 12 <18b>  - field 2: 18 bytes command data
    // 18 <varint>  - field 3: player_id (OUTSIDE field 2!)
    // 20 <varint>  - field 4: session_id (OUTSIDE field 2!)
    let mut ban_packet = Vec::with_capacity(40);

    // Header
    ban_packet.extend_from_slice(&[0x08, 0x01]);  // Message type 1

    // Field 2: command data (fixed 18 bytes)
    ban_packet.push(0x12);  // Field 2 tag
    ban_packet.push(0x12);  // Length = 18 bytes

    // Padding (4 bytes)
    ban_packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // Timestamp/Counter (4 bytes) - KICK packets ALWAYS use fixed value 0x000ef6a0 = 980640
    // This is NOT a dynamic timestamp - it's a special fixed value for KICK commands
    ban_packet.extend_from_slice(&[0xa0, 0xf6, 0x0e, 0x00]);
    log!("Using KICK fixed timestamp: 980640 (0x000ef6a0)");

    // Sequence numbers - KICK packets always use fixed seq1=08, seq2=02
    // The game expects these exact values for kick commands
    ban_packet.extend_from_slice(&[0x08, 0x00]);  // seq1 = 08 (fixed for KICK)
    ban_packet.extend_from_slice(&[0x02, 0x00]); // seq2 = 02 (fixed)

    // Command type = KICK (01 00) (2 bytes)
    ban_packet.extend_from_slice(&[0x01, 0x00]);

    // Flags (2 bytes)
    ban_packet.extend_from_slice(&[0x00, 0x00]);

    // Command code = 334 (4e 01) (2 bytes)
    ban_packet.extend_from_slice(&[0x4e, 0x01]);

    // Field 3: Player ID (OUTSIDE field 2)
    ban_packet.push(0x18);
    ban_packet.extend_from_slice(&player_id);

    // Field 4: Session ID (OUTSIDE field 2)
    ban_packet.push(0x20);
    ban_packet.extend_from_slice(&session_id);

    // Field 11: Confirmation/Ack data (5a 02 28 01)
    ban_packet.extend_from_slice(&[0x5a, 0x02, 0x28, 0x01]);

    log!("Built BAN packet ({} bytes): {:02x?}", ban_packet.len(), &ban_packet);

    let original_ptr = ORIGINAL_WSASENDTO.load(Ordering::SeqCst);
    if original_ptr.is_null() {
        log!("Cannot send BAN: WSASENDTO not hooked");
        return;
    }

    let original: WSASendToFn = std::mem::transmute(original_ptr);

    // Always include the actual source address of the detected packet first.
    // This is the relay address the opponent is actually using right now.
    let mut peer_addrs: Vec<SockAddrIn> = Vec::new();
    peer_addrs.push(*addr);

    let src_ip = ip_to_string(addr.sin_addr);
    let src_port = u16::from_be(addr.sin_port);
    log!("BAN primary target (packet source): {}:{}", src_ip, src_port);

    // Also collect active peers from LatencyData (skip duplicates)
    let latency_ptr = LATENCY_DATA.load(Ordering::SeqCst);
    if !latency_ptr.is_null() {
        let data = &*latency_ptr;
        for i in 0..MAX_PEERS {
            if data.peers[i].active == 1 {
                let peer_addr = SockAddrIn {
                    sin_family: 2, // AF_INET
                    sin_port: data.peers[i].port.to_be(),
                    sin_addr: data.peers[i].ip_addr,
                    sin_zero: [0; 8],
                };
                // Skip if same as the source address already added
                let is_dup = peer_addrs.iter().any(|existing|
                    existing.sin_addr == peer_addr.sin_addr && existing.sin_port == peer_addr.sin_port
                );
                if !is_dup {
                    peer_addrs.push(peer_addr);
                }
            }
        }
    }

    log!("Sending BAN to {} addresses (including packet source)", peer_addrs.len());

    let buf = WsaBuf {
        len: ban_packet.len() as u32,
        buf: ban_packet.as_ptr() as *mut u8,
    };

    let mut bytes_sent: u32 = 0;

    // Send BAN packet to ALL peers (like manual kick does)
    for peer_addr in &peer_addrs {
        let ip_str = ip_to_string(peer_addr.sin_addr);
        let port = u16::from_be(peer_addr.sin_port);

        for i in 0..3 {
            let result = original(
                socket,
                &buf,
                1,
                &mut bytes_sent,
                0,
                peer_addr,
                std::mem::size_of::<SockAddrIn>() as i32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            log!("Auto-BAN packet {} sent to {}:{}, result={}", i + 1, ip_str, port, result);
        }
    }

    // Also send simple disconnect to all peers
    let disconnect_packet: [u8; 2] = [0x08, 0x03];
    let buf2 = WsaBuf {
        len: disconnect_packet.len() as u32,
        buf: disconnect_packet.as_ptr() as *mut u8,
    };

    for peer_addr in &peer_addrs {
        let ip_str = ip_to_string(peer_addr.sin_addr);
        let port = u16::from_be(peer_addr.sin_port);

        let result = original(
            socket,
            &buf2,
            1,
            &mut bytes_sent,
            0,
            peer_addr,
            std::mem::size_of::<SockAddrIn>() as i32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        log!("Disconnect packet sent to {}:{}, result={}", ip_str, port, result);
    }
}

// Process pending kicks - called on each packet to check if we need to send more kicks
unsafe fn process_pending_kicks() {
    let should_kick = {
        let pending = PENDING_KICK.lock();
        if let Some(ref p) = *pending {
            if p.kicks_sent < TOTAL_KICKS {
                let elapsed = p.last_kick_time.elapsed().as_millis() as u64;
                elapsed >= KICK_INTERVAL_MS
            } else {
                false
            }
        } else {
            false
        }
    };

    if should_kick {
        let (socket, addr, kick_num) = {
            let mut pending = PENDING_KICK.lock();
            if let Some(ref mut p) = *pending {
                p.kicks_sent += 1;
                p.last_kick_time = std::time::Instant::now();
                (p.socket, p.addr, p.kicks_sent)
            } else {
                return;
            }
        };

        log!("=== KICK {}/{} (after {}s) ===", kick_num, TOTAL_KICKS, kick_num - 1);
        send_ban_packet(socket, &addr);

        // Clear pending if all kicks sent
        if kick_num >= TOTAL_KICKS {
            let mut pending = PENDING_KICK.lock();
            *pending = None;
            log!("=== All {} kicks sent, clearing pending ===", TOTAL_KICKS);
        }
    }
}

fn parse_kick_target_port(data: &[u8]) -> Option<u16> {
    if data.len() < 6 || data[2] != 0x12 {
        return None;
    }

    let msg_len = data[3] as usize;
    if data.len() < 4 + msg_len {
        return None;
    }

    let msg = &data[4..4 + msg_len];
    let mut pos = 0;

    while pos < msg.len() {
        let tag = msg[pos];
        pos += 1;

        let field_num = tag >> 3;
        let wire_type = tag & 0x07;

        if wire_type == 0 {
            let mut value: u64 = 0;
            let mut shift = 0;
            while pos < msg.len() {
                let b = msg[pos];
                pos += 1;
                value |= ((b & 0x7F) as u64) << shift;
                if b & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }

            if field_num == 4 {
                return Some(value as u16);
            }
        } else {
            break;
        }
    }

    None
}

// ============================================================================
// Hook Implementations
// ============================================================================

// Sanitize PNM: packet by replacing spaces (0x20) with underscores (0x5F)
// This prevents crashes when nicknames contain spaces, which get escaped to \032 by mDNS
unsafe fn sanitize_pnm_packet(buf: *mut u8, len: usize) -> bool {
    let data = std::slice::from_raw_parts_mut(buf, len);

    // Look for "PNM:" pattern (50 4e 4d 3a)
    for i in 0..len.saturating_sub(4) {
        if data[i] == 0x50 && data[i+1] == 0x4e && data[i+2] == 0x4d && data[i+3] == 0x3a {
            // Found PNM:, now sanitize the nickname part (after "PNM:")
            let nickname_start = i + 4;
            let mut modified = false;

            for j in nickname_start..len {
                // Stop at null terminator or non-printable characters
                if data[j] == 0x00 || data[j] < 0x20 {
                    break;
                }
                // Replace space (0x20) with underscore (0x5F)
                if data[j] == 0x20 {
                    log!("SANITIZE: Replacing space at offset {} with underscore", j);
                    data[j] = 0x5F; // '_'
                    modified = true;
                }
            }

            if modified {
                log!("PNM packet sanitized: spaces replaced with underscores");
                return true;
            }
        }
    }
    false
}

// Sanitize game protocol nickname packet (WSARECVFROM)
// Pattern: [08, 01, 12, 20, ..., 00, 07, ff, 00, <nickname with spaces>, 00, ...]
// Nickname field at offset 20 after pattern [00, 07, ff, 00]
unsafe fn sanitize_game_nickname_packet(buf: *mut u8, len: usize) -> bool {
    if len < 25 {
        return false;
    }

    let data = std::slice::from_raw_parts_mut(buf, len);

    // Check for game nickname packet pattern: 08 01 12 20 ... 00 07 ff 00
    if data[0] != 0x08 || data[1] != 0x01 || data[2] != 0x12 {
        return false;
    }

    // Look for pattern [00, 07, ff, 00] which precedes nickname
    for i in 4..len.saturating_sub(5) {
        if data[i] == 0x00 && data[i+1] == 0x07 && data[i+2] == 0xff && data[i+3] == 0x00 {
            // Found the pattern, nickname starts at i+4
            let nickname_start = i + 4;
            let mut modified = false;

            for j in nickname_start..len {
                // Stop at null terminator
                if data[j] == 0x00 {
                    break;
                }
                // Replace space (0x20) with underscore (0x5F)
                if data[j] == 0x20 {
                    log!("SANITIZE GAME: Replacing space at offset {} with underscore", j);
                    data[j] = 0x5F; // '_'
                    modified = true;
                }
            }

            if modified {
                log!("Game nickname packet sanitized: spaces replaced with underscores");
                return true;
            }
        }
    }
    false
}

// ============================================================================
// Map Scan Skip - CreateFile hooks (block Maps directory open)
// ============================================================================

/// Convert a wide (UTF-16) null-terminated string pointer to a lowercase Rust String.
unsafe fn wstr_to_lowercase(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    String::from_utf16_lossy(slice).to_lowercase()
}

/// Convert an ANSI (null-terminated u8) string pointer to a lowercase Rust String.
unsafe fn astr_to_lowercase(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    String::from_utf8_lossy(slice).to_lowercase()
}

/// Check if the path is inside the StarCraft Maps directory (directory itself OR files within).
/// Blocks both directory opens and individual file reads to prevent the entire scan chain.
/// The directory listing (NtQueryDirectoryFile) bypasses our hook, so StarCraft still gets
/// the file list — but every subsequent CreateFileW for individual .scx/.scm files will fail
/// instantly, making the scan complete in milliseconds instead of seconds.
fn is_maps_scan_path(path: &str) -> bool {
    // Match: ...\starcraft\maps\download\anything  or  ...\starcraft\maps\anything.scx/.scm
    // But NOT: ...\starcraft\maps  (just the directory path with no child — let that through)
    if let Some(pos) = path.find("starcraft\\maps").or_else(|| path.find("starcraft/maps")) {
        let after = &path[pos + "starcraft/maps".len()..];
        // Has content after "starcraft\maps" → it's a file or subdirectory access
        return !after.is_empty() && after != "\\" && after != "/";
    }
    false
}

extern "system" {
    fn SetLastError(dwErrCode: u32);
}
const ERROR_FILE_NOT_FOUND: u32 = 2;
const GENERIC_WRITE: u32 = 0x40000000;

/// Stores the lowercase filename of the current game map (whitelisted for read/write).
/// Set when StarCraft writes (saves) a downloaded map file.
static ALLOWED_MAP_FILE: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Extract just the filename from a full path (lowercase).
fn extract_filename(path: &str) -> &str {
    path.rsplit(|c: char| c == '\\' || c == '/').next().unwrap_or("")
}

unsafe extern "system" fn hooked_create_file_w(
    lp_file_name: *const u16,
    dw_desired_access: u32,
    dw_share_mode: u32,
    lp_security_attributes: *mut c_void,
    dw_creation_disposition: u32,
    dw_flags_and_attributes: u32,
    h_template_file: *mut c_void,
) -> *mut c_void {
    let path = wstr_to_lowercase(lp_file_name);

    if is_maps_scan_path(&path) {
        let filename = extract_filename(&path);

        let should_block = if dw_desired_access & GENERIC_WRITE != 0 {
            // WRITE access = saving downloaded map from CDN → allow + whitelist
            log!("MAP WRITE ALLOWED (whitelisted): {}", filename);
            *ALLOWED_MAP_FILE.lock() = Some(filename.to_string());
            false
        } else {
            // READ access: allow only if this file was whitelisted
            let is_whitelisted = ALLOWED_MAP_FILE.lock().as_deref() == Some(filename);
            if is_whitelisted {
                log!("MAP READ ALLOWED (whitelisted): {}", filename);
            }
            !is_whitelisted
        };

        if should_block {
            SetLastError(ERROR_FILE_NOT_FOUND);
            return -1isize as *mut c_void; // INVALID_HANDLE_VALUE
        }
    }

    let original: CreateFileWFn =
        std::mem::transmute(ORIGINAL_CREATE_FILE_W.load(Ordering::SeqCst));
    original(
        lp_file_name, dw_desired_access, dw_share_mode,
        lp_security_attributes, dw_creation_disposition,
        dw_flags_and_attributes, h_template_file,
    )
}

unsafe extern "system" fn hooked_create_file_a(
    lp_file_name: *const u8,
    dw_desired_access: u32,
    dw_share_mode: u32,
    lp_security_attributes: *mut c_void,
    dw_creation_disposition: u32,
    dw_flags_and_attributes: u32,
    h_template_file: *mut c_void,
) -> *mut c_void {
    let path = astr_to_lowercase(lp_file_name);

    if is_maps_scan_path(&path) {
        let filename = extract_filename(&path);

        let should_block = if dw_desired_access & GENERIC_WRITE != 0 {
            log!("MAP WRITE ALLOWED (whitelisted): {}", filename);
            *ALLOWED_MAP_FILE.lock() = Some(filename.to_string());
            false
        } else {
            let is_whitelisted = ALLOWED_MAP_FILE.lock().as_deref() == Some(filename);
            if is_whitelisted {
                log!("MAP READ ALLOWED (whitelisted): {}", filename);
            }
            !is_whitelisted
        };

        if should_block {
            SetLastError(ERROR_FILE_NOT_FOUND);
            return -1isize as *mut c_void; // INVALID_HANDLE_VALUE
        }
    }

    let original: CreateFileAFn =
        std::mem::transmute(ORIGINAL_CREATE_FILE_A.load(Ordering::SeqCst));
    original(
        lp_file_name, dw_desired_access, dw_share_mode,
        lp_security_attributes, dw_creation_disposition,
        dw_flags_and_attributes, h_template_file,
    )
}

// ============================================================================
// Winsock Hook Functions
// ============================================================================

/// Try to extract readable text from a WebSocket frame (server-side: unmasked).
/// Returns Some(text) if the data looks like a WS text frame with JSON.
fn try_extract_ws_text(data: &[u8]) -> Option<String> {
    if data.len() < 2 {
        return None;
    }
    let opcode = data[0] & 0x0F;
    if opcode != 1 {
        return None;
    }
    let masked = (data[1] & 0x80) != 0;
    let mut payload_len = (data[1] & 0x7F) as usize;
    let mut offset = 2usize;

    if payload_len == 126 {
        if data.len() < 4 { return None; }
        payload_len = ((data[2] as usize) << 8) | (data[3] as usize);
        offset = 4;
    } else if payload_len == 127 {
        if data.len() < 10 { return None; }
        payload_len = 0;
        for i in 0..8 {
            payload_len = (payload_len << 8) | (data[2 + i] as usize);
        }
        offset = 10;
    }

    let mask_key = if masked {
        if data.len() < offset + 4 { return None; }
        let key = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
        offset += 4;
        Some(key)
    } else {
        None
    };

    if data.len() < offset + payload_len || payload_len == 0 {
        return None;
    }

    let mut payload = data[offset..offset + payload_len].to_vec();
    if let Some(key) = mask_key {
        for i in 0..payload.len() {
            payload[i] ^= key[i % 4];
        }
    }

    match String::from_utf8(payload) {
        Ok(s) if s.contains('{') || s.contains("endpoint") => {
            let truncated = if s.len() > 500 { format!("{}...", &s[..500]) } else { s };
            Some(truncated)
        }
        _ => None,
    }
}

unsafe extern "system" fn hooked_recv(socket: SOCKET, buf: *mut u8, len: i32, flags: i32) -> i32 {
    let original: RecvFn = std::mem::transmute(ORIGINAL_RECV.load(Ordering::SeqCst));
    let result = original(socket, buf, len, flags);

    if result > 0 && !buf.is_null() {
        // Sanitize PNM: packets BEFORE the game processes them
        // This prevents crash from space characters in nicknames (mDNS \032 escape issue)
        sanitize_pnm_packet(buf, result as usize);

        let data = std::slice::from_raw_parts(buf, result as usize);
        log!("[RECV] socket={} len={} data={}",
            socket, result, format_hex(data));

        // Extract nickname from PNM: pattern (for logging only)
        if let Some(nickname) = extract_nickname_from_pnm(data) {
            log!("NICKNAME in PNM packet: {}", nickname);
        }

        // Decode WebSocket text frames
        if let Some(ws_text) = try_extract_ws_text(data) {
            log!("[WS RECV] socket={} text={}", socket, ws_text);
        }
    }

    result
}

unsafe extern "system" fn hooked_send(socket: SOCKET, buf: *const u8, len: i32, flags: i32) -> i32 {
    if len > 0 && !buf.is_null() {
        let data = std::slice::from_raw_parts(buf, len as usize);
        log!("[SEND] socket={} len={} data={}",
            socket, len, format_hex(data));

        // Decode WebSocket text frames
        if let Some(ws_text) = try_extract_ws_text(data) {
            log!("[WS SEND] socket={} text={}", socket, ws_text);
        }
    }

    let original: SendFn = std::mem::transmute(ORIGINAL_SEND.load(Ordering::SeqCst));
    original(socket, buf, len, flags)
}

unsafe extern "system" fn hooked_sendto(
    socket: SOCKET,
    buf: *const u8,
    len: i32,
    flags: i32,
    to: *const SockAddrIn,
    tolen: i32,
) -> i32 {
    if len > 0 && !buf.is_null() && !to.is_null() {
        let data = std::slice::from_raw_parts(buf, len as usize);
        let addr = &*to;
        let dest_ip = ip_to_string(addr.sin_addr);
        let dest_port = u16::from_be(addr.sin_port);

        log!("[SENDTO] socket={} dest={}:{} len={} data={}",
            socket, dest_ip, dest_port, len, format_hex(data));

        if is_kick_packet(data) {
            let target_port = parse_kick_target_port(data).unwrap_or(0);
            log!("=== KICK PACKET DETECTED === target_port={}", target_port);

            let mut kick_data = KICK_DATA.lock();
            kick_data.socket = socket;
            kick_data.dest_addr = *addr;
            kick_data.last_kick_packet = data.to_vec();
        }
    }

    let original: SendToFn = std::mem::transmute(ORIGINAL_SENDTO.load(Ordering::SeqCst));
    let result = original(socket, buf, len, flags, to, tolen);

    // Process kick queue
    let mut queue = KICK_QUEUE.lock();
    if !queue.is_empty() {
        let ports: Vec<u16> = queue.drain(..).collect();
        drop(queue);

        for _port in ports {
            let kick_data = KICK_DATA.lock();
            if kick_data.socket != 0 && !kick_data.last_kick_packet.is_empty() {
                log!("Sending queued kick packet");
                original(
                    kick_data.socket,
                    kick_data.last_kick_packet.as_ptr(),
                    kick_data.last_kick_packet.len() as i32,
                    0,
                    &kick_data.dest_addr,
                    std::mem::size_of::<SockAddrIn>() as i32,
                );
            }
        }
    }

    result
}

unsafe extern "system" fn hooked_recvfrom(
    socket: SOCKET,
    buf: *mut u8,
    len: i32,
    flags: i32,
    from: *mut SockAddrIn,
    fromlen: *mut i32,
) -> i32 {
    let original: RecvFromFn = std::mem::transmute(ORIGINAL_RECVFROM.load(Ordering::SeqCst));
    let result = original(socket, buf, len, flags, from, fromlen);

    if result > 0 && !buf.is_null() {
        let data = std::slice::from_raw_parts(buf, result as usize);

        if !from.is_null() {
            let addr = &*from;
            let src_ip = ip_to_string(addr.sin_addr);
            let src_port = u16::from_be(addr.sin_port);
            log!("[RECVFROM] socket={} from={}:{} len={} data={}",
                socket, src_ip, src_port, result, format_hex(data));
        } else {
            log!("[RECVFROM] socket={} len={} data={}",
                socket, result, format_hex(data));
        }
    }

    result
}

unsafe extern "system" fn hooked_connect(socket: SOCKET, addr: *const SockAddrIn, addrlen: i32) -> i32 {
    if !addr.is_null() {
        let addr_ref = &*addr;
        let ip = ip_to_string(addr_ref.sin_addr);
        let port = u16::from_be(addr_ref.sin_port);
        log!("[CONNECT] socket={} dest={}:{}", socket, ip, port);
    }

    let original: ConnectFn = std::mem::transmute(ORIGINAL_CONNECT.load(Ordering::SeqCst));
    original(socket, addr, addrlen)
}

unsafe extern "system" fn hooked_closesocket(socket: SOCKET) -> i32 {
    log!("[CLOSESOCKET] socket={}", socket);

    let original: CloseSocketFn = std::mem::transmute(ORIGINAL_CLOSESOCKET.load(Ordering::SeqCst));
    original(socket)
}

unsafe extern "system" fn hooked_wsasendto(
    socket: SOCKET,
    buffers: *const WsaBuf,
    buffer_count: u32,
    bytes_sent: *mut u32,
    flags: u32,
    to: *const SockAddrIn,
    tolen: i32,
    overlapped: *mut c_void,
    completion: *mut c_void,
) -> i32 {
    // Log before sending
    if !buffers.is_null() && buffer_count > 0 && !to.is_null() {
        let buf = &*buffers;
        if !buf.buf.is_null() && buf.len > 0 {
            let data = std::slice::from_raw_parts(buf.buf, buf.len as usize);
            let addr = &*to;
            let dest_ip = ip_to_string(addr.sin_addr);
            let dest_port = u16::from_be(addr.sin_port);

            log!("[WSASENDTO] socket={} dest={}:{} len={} data={}",
                socket, dest_ip, dest_port, buf.len, format_hex(data));

            // Record send time for latency calculation
            record_send_time(addr.sin_addr, dest_port);

            // Try to extract my session info from outgoing packets (pattern: 08 01 18 ...)
            try_extract_my_session_info(data);

            // Update timestamp from outgoing packets
            update_timestamp_from_packet(data);

            // Try to extract room name from packet
            if let Some(room_name) = extract_room_name_from_packet(data) {
                log!("ROOM NAME DETECTED: '{}'", room_name);
                update_room_name(&room_name);
            }

            // Try to extract map name from packet
            if let Some(map_name) = extract_map_name_from_packet(data) {
                log!("MAP NAME DETECTED: '{}'", map_name);
                update_map_name(&map_name);
            }

            // Extract room info from outgoing packets too (Room Info can appear in SENDTO)
            if let Some((room_name, map_name, host_name, room_id)) = extract_room_info_from_packet(data) {
                log!("=== ROOM INFO (SENDTO) === room='{}' map='{}' host='{}' room_id={}",
                    room_name, map_name, host_name, format_hex(&room_id));
                clear_room_users();
                clear_latency_peers();
                update_room_info(&room_name, &map_name, &host_name, &room_id);
            }

            // Extract user info from outgoing packets too
            if let Some((battletag, nickname)) = extract_user_info_from_441_packet(data) {
                log!("USER INFO (SENDTO): battletag={} nickname={}", battletag, nickname);
                add_room_user_with_info(&battletag, &nickname, &dest_ip, dest_port);
            }

            if is_kick_packet(data) {
                let target_port = parse_kick_target_port(data).unwrap_or(0);
                log!("=== KICK PACKET DETECTED (WSA) === target_port={}", target_port);

                let mut kick_data = KICK_DATA.lock();
                kick_data.socket = socket;
                kick_data.dest_addr = *addr;
                kick_data.last_kick_packet = data.to_vec();
            }
        }
    }

    let original: WSASendToFn = std::mem::transmute(ORIGINAL_WSASENDTO.load(Ordering::SeqCst));
    original(socket, buffers, buffer_count, bytes_sent, flags, to, tolen, overlapped, completion)
}

unsafe extern "system" fn hooked_wsarecvfrom(
    socket: SOCKET,
    buffers: *const WsaBuf,
    buffer_count: u32,
    bytes_received: *mut u32,
    flags: *mut u32,
    from: *mut SockAddrIn,
    fromlen: *mut i32,
    overlapped: *mut c_void,
    completion: *mut c_void,
) -> i32 {
    let original: WSARecvFromFn = std::mem::transmute(ORIGINAL_WSARECVFROM.load(Ordering::SeqCst));
    let result = original(socket, buffers, buffer_count, bytes_received, flags, from, fromlen, overlapped, completion);

    // Log after receiving (for non-overlapped calls)
    if result == 0 && !buffers.is_null() && buffer_count > 0 && !bytes_received.is_null() {
        let received = *bytes_received;
        if received > 0 {
            let buf = &*buffers;
            if !buf.buf.is_null() {
                // Sanitize game nickname packets BEFORE processing
                // This prevents crash from space characters in nicknames
                sanitize_game_nickname_packet(buf.buf, received as usize);

                let data = std::slice::from_raw_parts(buf.buf, received as usize);

                if !from.is_null() {
                    let addr = &*from;
                    let src_ip = ip_to_string(addr.sin_addr);
                    let src_port = u16::from_be(addr.sin_port);
                    log!("[WSARECVFROM] socket={} from={}:{} len={} data={}",
                        socket, src_ip, src_port, received, format_hex(data));

                    // Record recv time for latency calculation
                    record_recv_time(addr.sin_addr, src_port);

                    // Check for disconnect packet (peer leaving)
                    if is_disconnect_packet(data) {
                        log!("=== DISCONNECT PACKET from {}:{} ===", src_ip, src_port);
                        remove_room_user_by_ip(addr.sin_addr);
                        remove_latency_by_ip(addr.sin_addr);
                    }

                    // Check for auto-ban target
                    if let Some(matched_battletag) = contains_ban_target(data) {
                        log!("=== AUTO-BAN TARGET DETECTED: {} from {}:{} ===",
                            matched_battletag,
                            src_ip, src_port);

                        // Check if we already have a pending kick for this target
                        let should_start_new = {
                            let pending = PENDING_KICK.lock();
                            pending.is_none() || pending.as_ref().map_or(true, |p| p.kicks_sent >= TOTAL_KICKS)
                        };

                        if should_start_new {
                            // Send first kick immediately
                            log!("=== KICK 1/{} (immediate) ===", TOTAL_KICKS);
                            send_ban_packet(socket, addr);

                            // Update statistics
                            update_autoban_stats(&matched_battletag);

                            // Store pending kick info for remaining kicks
                            let mut pending = PENDING_KICK.lock();
                            *pending = Some(PendingKick {
                                socket,
                                addr: *addr,
                                detected_time: std::time::Instant::now(),
                                kicks_sent: 1,
                                last_kick_time: std::time::Instant::now(),
                            });
                        }
                    }

                    // Process pending kicks (send at 1s, 2s, 3s intervals)
                    process_pending_kicks();

                    // Extract room info from room join packet (clears existing users)
                    if let Some((room_name, map_name, host_name, room_id)) = extract_room_info_from_packet(data) {
                        log!("=== NEW ROOM JOINED === room='{}' map='{}' host='{}' room_id={}",
                            room_name, map_name, host_name, format_hex(&room_id));
                        clear_room_users();
                        clear_latency_peers();
                        update_room_info(&room_name, &map_name, &host_name, &room_id);
                    }

                    // Force parsing disabled - packet structure too complex/uncertain
                    // if let Some(forces) = extract_force_info_from_packet(data) {
                    //     log!("=== FORCE INFO === {} forces detected", forces.len());
                    //     update_force_info(&forces);
                    // }

                    // Extract battletag and nickname from 441-byte packet
                    if let Some((battletag, nickname)) = extract_user_info_from_441_packet(data) {
                        log!("USER INFO DETECTED: battletag={} nickname={} from {}:{}",
                            battletag, nickname, src_ip, src_port);
                        add_room_user_with_info(&battletag, &nickname, &src_ip, src_port);

                        // Failsafe: re-check blacklist cache for this user
                        let matched_target = {
                            let cache = BLACKLIST_CACHE.lock();
                            log!("AUTO-BAN DEBUG: User detected='{}', BanList(len={}):", battletag, cache.len());
                            let mut found = None;
                            for (i, target) in cache.iter().enumerate() {
                                if let Ok(t_str) = std::str::from_utf8(target) {
                                    let is_match = battletag.eq_ignore_ascii_case(t_str);
                                    log!("  [{}] '{}' (match={})", i, t_str, is_match);
                                    if is_match {
                                        found = Some(t_str.to_string());
                                        break;
                                    }
                                }
                            }
                            found
                        };
                        if let Some(ref matched) = matched_target {
                            log!("  !!! TRIGGERING FAILSAFE KICK !!!");
                            send_ban_packet(socket, addr);
                            update_autoban_stats(matched);
                        }
                    }
                } else {
                    log!("[WSARECVFROM] socket={} len={} data={}",
                        socket, received, format_hex(data));
                }
            }
        }
    }

    result
}

// ============================================================================
// Hook Installation
// ============================================================================

fn install_hooks() -> bool {
    unsafe {
        let ws2_32 = match GetModuleHandleA(PCSTR(b"ws2_32.dll\0".as_ptr())) {
            Ok(h) => h,
            Err(_) => {
                log!("Failed to get ws2_32.dll handle");
                return false;
            }
        };

        // Install recv hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"recv\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_recv as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_RECV.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("recv hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create recv hook: {:?}", e),
            }
        }

        // Install send hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"send\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_send as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_SEND.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("send hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create send hook: {:?}", e),
            }
        }

        // Install sendto hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"sendto\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_sendto as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_SENDTO.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("sendto hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create sendto hook: {:?}", e),
            }
        }

        // Install recvfrom hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"recvfrom\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_recvfrom as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_RECVFROM.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("recvfrom hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create recvfrom hook: {:?}", e),
            }
        }

        // Install connect hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"connect\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_connect as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_CONNECT.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("connect hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create connect hook: {:?}", e),
            }
        }

        // Install closesocket hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"closesocket\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_closesocket as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_CLOSESOCKET.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("closesocket hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create closesocket hook: {:?}", e),
            }
        }

        // Install WSASendTo hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"WSASendTo\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_wsasendto as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_WSASENDTO.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("WSASendTo hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create WSASendTo hook: {:?}", e),
            }
        }

        // Install WSARecvFrom hook
        if let Some(target) = GetProcAddress(ws2_32, PCSTR(b"WSARecvFrom\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_wsarecvfrom as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_WSARECVFROM.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("WSARecvFrom hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create WSARecvFrom hook: {:?}", e),
            }
        }

        // ====================================================================
        // kernel32.dll hooks - Map scan skip
        // ====================================================================
        let kernel32 = match GetModuleHandleA(PCSTR(b"kernel32.dll\0".as_ptr())) {
            Ok(h) => h,
            Err(_) => {
                log!("Failed to get kernel32.dll handle (map scan skip disabled)");
                HOOKS_INSTALLED.store(true, Ordering::SeqCst);
                return true;
            }
        };

        // Install CreateFileW hook
        if let Some(target) = GetProcAddress(kernel32, PCSTR(b"CreateFileW\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_create_file_w as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_CREATE_FILE_W.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("CreateFileW hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create CreateFileW hook: {:?}", e),
            }
        }

        // Install CreateFileA hook
        if let Some(target) = GetProcAddress(kernel32, PCSTR(b"CreateFileA\0".as_ptr())) {
            match MinHook::create_hook(target as *mut c_void, hooked_create_file_a as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_CREATE_FILE_A.store(original, Ordering::SeqCst);
                    let _ = MinHook::enable_hook(target as *mut c_void);
                    log!("CreateFileA hook installed at {:p}", target);
                }
                Err(e) => log!("Failed to create CreateFileA hook: {:?}", e),
            }
        }

        HOOKS_INSTALLED.store(true, Ordering::SeqCst);
        log!("All hooks installed successfully (including map scan skip)");
        true
    }
}

fn disable_hooks() {
    unsafe {
        let _ = MinHook::disable_all_hooks();
    }
    log!("All hooks disabled");
}

// ============================================================================
// Exported Functions
// ============================================================================

#[no_mangle]
pub extern "C" fn QueueKick(target_port: u16) {
    log!("QueueKick called for port {}", target_port);
    KICK_QUEUE.lock().push(target_port);
}

#[no_mangle]
pub extern "C" fn SendKickNow() -> bool {
    let kick_data = KICK_DATA.lock();

    if kick_data.socket == 0 || kick_data.last_kick_packet.is_empty() {
        log!("SendKickNow: No kick data captured yet");
        return false;
    }

    log!("SendKickNow: Sending {} bytes", kick_data.last_kick_packet.len());

    let original_ptr = ORIGINAL_SENDTO.load(Ordering::SeqCst);
    if original_ptr.is_null() {
        return false;
    }

    unsafe {
        let original: SendToFn = std::mem::transmute(original_ptr);
        let result = original(
            kick_data.socket,
            kick_data.last_kick_packet.as_ptr(),
            kick_data.last_kick_packet.len() as i32,
            0,
            &kick_data.dest_addr,
            std::mem::size_of::<SockAddrIn>() as i32,
        );
        log!("SendKickNow: Result = {}", result);
        result > 0
    }
}

#[no_mangle]
pub extern "C" fn IsHookInstalled() -> bool {
    HOOKS_INSTALLED.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn HasKickData() -> bool {
    let kick_data = KICK_DATA.lock();
    kick_data.socket != 0 && !kick_data.last_kick_packet.is_empty()
}

// ============================================================================
// Drop Timer (dis-zero) Exported Functions
// ============================================================================

#[no_mangle]
pub extern "C" fn IsDropTimerEnabled() -> bool {
    let config_ptr = DROP_TIMER_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return false;
    }
    unsafe { (*config_ptr).enabled == 1 }
}

#[no_mangle]
pub extern "C" fn SetDropTimerEnabled(enabled: bool) {
    let config_ptr = DROP_TIMER_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return;
    }
    unsafe {
        (*config_ptr).enabled = if enabled { 1 } else { 0 };
        log!("[DROP-TIMER] Enabled set to: {}", enabled);
    }
}

#[no_mangle]
pub extern "C" fn GetDropTimerStatus() -> u32 {
    let config_ptr = DROP_TIMER_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return 3; // error
    }
    unsafe { (*config_ptr).status }
}

#[no_mangle]
pub extern "C" fn GetDropTimerResetCount() -> u32 {
    let config_ptr = DROP_TIMER_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return 0;
    }
    unsafe { (*config_ptr).total_resets }
}

#[no_mangle]
pub extern "C" fn GetCurrentTimerValue() -> u32 {
    let config_ptr = DROP_TIMER_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return 0;
    }
    unsafe { (*config_ptr).current_timer_value }
}

#[no_mangle]
pub extern "C" fn IsVersionVerified() -> bool {
    let config_ptr = DROP_TIMER_CONFIG.load(Ordering::SeqCst);
    if config_ptr.is_null() {
        return false;
    }
    unsafe { (*config_ptr).version_verified == 1 }
}

// ============================================================================
// Command Thread
// ============================================================================

unsafe extern "system" fn command_thread(_param: *mut c_void) -> u32 {
    log!("Command thread started");

    let handle = match CreateFileMappingA(
        HANDLE(-1isize as *mut c_void),
        None,
        PAGE_READWRITE,
        0,
        std::mem::size_of::<SharedPacketData>() as u32,
        PCSTR(SHARED_MEMORY_NAME.as_ptr()),
    ) {
        Ok(h) => h,
        Err(_) => {
            log!("Failed to create shared memory");
            return 1;
        }
    };

    let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
    if view.Value.is_null() {
        log!("Failed to map shared memory");
        return 1;
    }

    let data_ptr = view.Value as *mut SharedPacketData;
    (*data_ptr).packet_count = 0;

    log!("Shared memory initialized");

    // Initialize auto-ban shared memory
    let autoban_handle = match CreateFileMappingA(
        HANDLE(-1isize as *mut c_void),
        None,
        PAGE_READWRITE,
        0,
        std::mem::size_of::<AutoBanConfig>() as u32,
        PCSTR(AUTOBAN_MEMORY_NAME.as_ptr()),
    ) {
        Ok(h) => h,
        Err(_) => {
            log!("Failed to create auto-ban shared memory");
            return 1;
        }
    };

    let autoban_view = MapViewOfFile(autoban_handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
    if autoban_view.Value.is_null() {
        log!("Failed to map auto-ban shared memory");
        return 1;
    }

    let autoban_ptr = autoban_view.Value as *mut AutoBanConfig;

    // Initialize with defaults (only if version is 0, meaning not initialized by sc-monitor)
    if (*autoban_ptr).version == 0 {
        (*autoban_ptr).version = 1;
        (*autoban_ptr).enabled = 0;  // Disabled by default
        (*autoban_ptr).target_count = 0;
        (*autoban_ptr).total_kicks = 0;
        (*autoban_ptr).successful_kicks = 0;
        (*autoban_ptr).last_kick_time = 0;
    }

    // Store the pointer globally
    AUTOBAN_CONFIG.store(autoban_ptr, Ordering::SeqCst);
    log!("Auto-ban shared memory initialized (enabled={}, targets={})",
        (*autoban_ptr).enabled, (*autoban_ptr).target_count);

    // Initialize room users shared memory
    let roomusers_handle = match CreateFileMappingA(
        HANDLE(-1isize as *mut c_void),
        None,
        PAGE_READWRITE,
        0,
        std::mem::size_of::<RoomUsersData>() as u32,
        PCSTR(ROOMUSERS_MEMORY_NAME.as_ptr()),
    ) {
        Ok(h) => h,
        Err(_) => {
            log!("Failed to create room users shared memory");
            return 1;
        }
    };

    let roomusers_view = MapViewOfFile(roomusers_handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
    if roomusers_view.Value.is_null() {
        log!("Failed to map room users shared memory");
        return 1;
    }

    let roomusers_ptr = roomusers_view.Value as *mut RoomUsersData;

    // Initialize with defaults
    (*roomusers_ptr).version = 1;
    (*roomusers_ptr).user_count = 0;
    (*roomusers_ptr).last_update = 0;
    (*roomusers_ptr).reserved = 0;
    for i in 0..MAX_ROOM_USERS {
        (*roomusers_ptr).users[i].active = 0;
        (*roomusers_ptr).users[i].player_id = 0;
        (*roomusers_ptr).users[i].nickname = [0; MAX_NICKNAME_LEN];
        (*roomusers_ptr).users[i].battletag = [0; MAX_BATTLETAG_LEN];
        (*roomusers_ptr).users[i].ip_address = [0; MAX_IP_LEN];
        (*roomusers_ptr).users[i].port = 0;
        (*roomusers_ptr).users[i].flags = 0;
    }

    // Store the pointer globally
    ROOM_USERS.store(roomusers_ptr, Ordering::SeqCst);
    log!("Room users shared memory initialized");

    // Initialize room info shared memory
    let roominfo_handle = match CreateFileMappingA(
        HANDLE(-1isize as *mut c_void),
        None,
        PAGE_READWRITE,
        0,
        std::mem::size_of::<RoomInfo>() as u32,
        PCSTR(ROOMINFO_MEMORY_NAME.as_ptr()),
    ) {
        Ok(h) => h,
        Err(_) => {
            log!("Failed to create room info shared memory");
            return 1;
        }
    };

    let roominfo_view = MapViewOfFile(roominfo_handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
    if roominfo_view.Value.is_null() {
        log!("Failed to map room info shared memory");
        return 1;
    }

    let roominfo_ptr = roominfo_view.Value as *mut RoomInfo;

    // Initialize with defaults
    (*roominfo_ptr).version = 1;
    (*roominfo_ptr).in_room = 0;
    (*roominfo_ptr).last_update = 0;
    (*roominfo_ptr).force_count = 0;
    (*roominfo_ptr).room_name = [0; MAX_ROOM_NAME_LEN];
    (*roominfo_ptr).map_name = [0; MAX_MAP_NAME_LEN];
    (*roominfo_ptr).host_name = [0; MAX_HOST_NAME_LEN];
    for i in 0..MAX_FORCES {
        (*roominfo_ptr).force_names[i] = [0; MAX_FORCE_NAME_LEN];
    }

    // Store the pointer globally
    ROOM_INFO.store(roominfo_ptr, Ordering::SeqCst);
    log!("Room info shared memory initialized");

    // Initialize latency shared memory
    let latency_handle = match CreateFileMappingA(
        HANDLE(-1isize as *mut c_void),
        None,
        PAGE_READWRITE,
        0,
        std::mem::size_of::<LatencyData>() as u32,
        PCSTR(LATENCY_MEMORY_NAME.as_ptr()),
    ) {
        Ok(h) => h,
        Err(_) => {
            log!("Failed to create latency shared memory");
            return 1;
        }
    };

    let latency_view = MapViewOfFile(latency_handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
    if latency_view.Value.is_null() {
        log!("Failed to map latency shared memory");
        return 1;
    }

    let latency_ptr = latency_view.Value as *mut LatencyData;

    // Initialize with defaults
    (*latency_ptr).version = 1;
    (*latency_ptr).peer_count = 0;
    (*latency_ptr).last_update = 0;
    (*latency_ptr).reserved = 0;
    for i in 0..MAX_PEERS {
        (*latency_ptr).peers[i].active = 0;
        (*latency_ptr).peers[i].ip_addr = 0;
        (*latency_ptr).peers[i].port = 0;
        (*latency_ptr).peers[i].last_send_time = 0;
        (*latency_ptr).peers[i].last_recv_time = 0;
        (*latency_ptr).peers[i].current_rtt_us = 0;
        (*latency_ptr).peers[i].avg_rtt_us = 0;
        (*latency_ptr).peers[i].sample_count = 0;
    }

    // Store the pointer globally
    LATENCY_DATA.store(latency_ptr, Ordering::SeqCst);
    log!("Latency shared memory initialized");

    // Initialize drop timer (dis-zero) shared memory
    let droptimer_handle = match CreateFileMappingA(
        HANDLE(-1isize as *mut c_void),
        None,
        PAGE_READWRITE,
        0,
        std::mem::size_of::<DropTimerConfig>() as u32,
        PCSTR(DROP_TIMER_MEMORY_NAME.as_ptr()),
    ) {
        Ok(h) => h,
        Err(_) => {
            log!("Failed to create drop timer shared memory");
            return 1;
        }
    };

    let droptimer_view = MapViewOfFile(droptimer_handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
    if droptimer_view.Value.is_null() {
        log!("Failed to map drop timer shared memory");
        return 1;
    }

    let droptimer_ptr = droptimer_view.Value as *mut DropTimerConfig;

    // Initialize drop timer config with defaults
    init_drop_timer_config(&mut *droptimer_ptr);

    // Store the pointer globally
    DROP_TIMER_CONFIG.store(droptimer_ptr, Ordering::SeqCst);
    log!("[DROP-TIMER] Shared memory initialized (dis-zero integrated)");
    log!("[DROP-TIMER] Expected version: {}", DEFAULT_VERSION);
    log!("[DROP-TIMER] 32-bit offset: 0x{:X}, 64-bit offset: 0x{:X}",
        DEFAULT_DROP_TIMER_OFFSET_32, DEFAULT_DROP_TIMER_OFFSET_64);

    // Initialize high-precision timer
    init_timer();

    // Load blacklist cache on startup
    refresh_blacklist_cache();
    log!("Blacklist cache loaded: {} entries", BLACKLIST_CACHE.lock().len());

    let mut blacklist_refresh_counter: u32 = 0;

    while COMMAND_THREAD_RUNNING.load(Ordering::SeqCst) {
        {
            let kick_data = KICK_DATA.lock();
            (*data_ptr).has_kick_data = if kick_data.socket != 0 { 1 } else { 0 };
        }

        // Reset drop timer (dis-zero functionality)
        reset_drop_timer();

        // Refresh blacklist cache from file every ~2 seconds (40 * 50ms)
        blacklist_refresh_counter += 1;
        if blacklist_refresh_counter >= 40 {
            blacklist_refresh_counter = 0;
            refresh_blacklist_cache();
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    log!("Command thread exiting");
    0
}

// ============================================================================
// DLL Entry Point
// ============================================================================

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(
    h_module: HMODULE,
    ul_reason_for_call: u32,
    _lp_reserved: *mut c_void,
) -> BOOL {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match ul_reason_for_call {
        DLL_PROCESS_ATTACH => {
            unsafe {
                let _ = DisableThreadLibraryCalls(h_module);
            }

            log!("=== SC Monitor Hook DLL v3.0 (with dis-zero) ===");
            log!("DLL_PROCESS_ATTACH: Initializing...");

            // Initialize timer BEFORE hooks (critical for latency measurement)
            init_timer();

            if install_hooks() {
                COMMAND_THREAD_RUNNING.store(true, Ordering::SeqCst);
                unsafe {
                    let _ = CreateThread(
                        None,
                        0,
                        Some(command_thread),
                        None,
                        THREAD_CREATION_FLAGS(0),
                        None,
                    );
                }
                log!("Initialization complete");
            } else {
                log!("Hook installation failed");
            }
        }
        DLL_PROCESS_DETACH => {
            log!("DLL_PROCESS_DETACH: Cleaning up...");
            COMMAND_THREAD_RUNNING.store(false, Ordering::SeqCst);
            disable_hooks();
        }
        _ => {}
    }

    TRUE
}
