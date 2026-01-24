import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { motion } from "framer-motion";
import { Settings, Power, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ConnectionChip } from "@/components/ConnectionChip";
import { RoomInfoCard } from "@/components/RoomInfoCard";
import { UserListTable, type RoomUser } from "@/components/UserListTable";
import { BlacklistPanel, type BlacklistEntry } from "@/components/BlacklistPanel";
import { AddBlacklistDialog } from "@/components/AddBlacklistDialog";
import { RoomHistoryPanel, type RoomHistoryEntry } from "@/components/RoomHistoryPanel";

interface RoomInfo {
  inRoom: boolean;
  roomId?: string;
  roomName: string;
  mapName: string;
  hostName?: string;
  forceNames?: string[];
}

interface PeerLatency {
  ipAddress: string;
  port: number;
  pingMs: number;
  avgPingMs: number;
  sampleCount: number;
}

function App() {
  const [roomUsers, setRoomUsers] = useState<RoomUser[]>([]);
  const [blacklist, setBlacklist] = useState<BlacklistEntry[]>([]);
  const [roomInfo, setRoomInfo] = useState<RoomInfo>({ inRoom: false, roomName: "", mapName: "" });
  const [peerLatencies, setPeerLatencies] = useState<PeerLatency[]>([]);
  const [_isConnected, setIsConnected] = useState(false); // Reserved for future use
  const [isDllInjected, setIsDllInjected] = useState(false);
  const [starcraftPid, setStarcraftPid] = useState<number | null>(null);
  const [isInjecting, setIsInjecting] = useState(false);
  const [injectStatus, setInjectStatus] = useState<string>("");
  const [autoInject, setAutoInject] = useState(true);
  const [selectedUser, setSelectedUser] = useState<RoomUser | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const lastInjectedPidRef = useRef<number | null>(null);

  // History state
  const [roomHistory, setRoomHistory] = useState<RoomHistoryEntry[]>([]);
  const prevRoomIdRef = useRef<string | null>(null);
  const currentHistoryTimestampRef = useRef<number | null>(null);

  // Fetch data on mount and periodically
  useEffect(() => {
    let isMounted = true;

    const fetchData = async () => {
      if (!isMounted) return;
      try {
        // Run all fetches in parallel to reduce blocking
        const [connected, users, list, info, latencies, pid] = await Promise.all([
          invoke<boolean>("is_connected"),
          invoke<RoomUser[]>("get_room_users"),
          invoke<BlacklistEntry[]>("get_blacklist"),
          invoke<RoomInfo>("get_room_info"),
          invoke<PeerLatency[]>("get_peer_latencies"),
          invoke<number | null>("find_starcraft_process"),
        ]);

        if (!isMounted) return;
        setIsConnected(connected);
        setRoomUsers(users);
        setBlacklist(list);
        setRoomInfo(info);
        setPeerLatencies(latencies);
        setStarcraftPid(pid);
      } catch (error) {
        console.error("Failed to fetch data:", error);
      }
    };

    fetchData();
    const interval = setInterval(fetchData, 2000); // Reduced from 1000ms to 2000ms
    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, []);

  // Load history on mount
  useEffect(() => {
    const loadHistory = async () => {
      try {
        const history = await invoke<RoomHistoryEntry[]>("get_room_history");
        setRoomHistory(history);
      } catch (error) {
        console.error("Failed to load history:", error);
      }
    };
    loadHistory();
  }, []);

  // Track room and accumulate users in history
  useEffect(() => {
    // Only process when in a room with valid roomId
    if (!roomInfo.inRoom || !roomInfo.roomId) {
      return;
    }

    const currentRoomId = roomInfo.roomId;
    const prevRoomId = prevRoomIdRef.current;

    // Check if this is a new room (roomId changed)
    if (prevRoomId !== currentRoomId) {
      console.log("[HISTORY] New room detected:", {
        prevRoomId,
        currentRoomId,
        roomName: roomInfo.roomName,
      });

      // Create new history entry for this room
      const timestamp = Date.now();
      const historyEntry: RoomHistoryEntry = {
        timestamp,
        roomInfo: {
          inRoom: true,
          roomId: roomInfo.roomId || "",
          roomName: roomInfo.roomName,
          mapName: roomInfo.mapName,
          hostName: roomInfo.hostName || "",
          forceNames: roomInfo.forceNames || [],
        },
        users: [...roomUsers], // Initial users
      };

      currentHistoryTimestampRef.current = timestamp;
      prevRoomIdRef.current = currentRoomId;

      invoke("add_room_history", { entry: historyEntry })
        .then(() => invoke<RoomHistoryEntry[]>("get_room_history"))
        .then((history) => setRoomHistory(history))
        .catch((error) => console.error("Failed to create history:", error));
    } else if (currentHistoryTimestampRef.current && roomUsers.length > 0) {
      // Same room - accumulate users (add new users, don't remove existing)
      const currentTimestamp = currentHistoryTimestampRef.current;

      setRoomHistory((prevHistory) => {
        const historyIndex = prevHistory.findIndex(h => h.timestamp === currentTimestamp);
        if (historyIndex === -1) return prevHistory;

        const currentEntry = prevHistory[historyIndex];
        const existingBattletags = new Set(currentEntry.users.map(u => u.battletag));

        // Add new users that don't exist in history
        const newUsers = roomUsers.filter(u => !existingBattletags.has(u.battletag));

        if (newUsers.length === 0) return prevHistory; // No new users

        console.log("[HISTORY] Adding new users to history:", newUsers.map(u => u.nickname));

        const updatedEntry: RoomHistoryEntry = {
          ...currentEntry,
          users: [...currentEntry.users, ...newUsers],
        };

        // Update in backend
        invoke("add_room_history", { entry: updatedEntry })
          .catch((error) => console.error("Failed to update history:", error));

        // Update local state
        const newHistory = [...prevHistory];
        newHistory[historyIndex] = updatedEntry;
        return newHistory;
      });
    }
  }, [roomInfo, roomUsers]);

  // Reset DLL injection status when StarCraft is not running
  useEffect(() => {
    if (!starcraftPid && isDllInjected) {
      console.log("[STATUS] StarCraft closed, resetting DLL injection status");
      setIsDllInjected(false);
      lastInjectedPidRef.current = null;
    }
  }, [starcraftPid, isDllInjected]);

  // Auto-inject DLL
  useEffect(() => {
    if (!autoInject) return;

    let isMounted = true;
    const checkAndInject = async () => {
      if (!isMounted || isInjecting) return;

      try {
        // No StarCraft process
        if (!starcraftPid) {
          if (lastInjectedPidRef.current !== null) {
            console.log("[AUTO-INJECT] StarCraft closed, resetting state");
            lastInjectedPidRef.current = null;
            setIsDllInjected(false);
          }
          return;
        }

        // New PID detected - completely reset state
        if (lastInjectedPidRef.current !== null && lastInjectedPidRef.current !== starcraftPid) {
          console.log(`[AUTO-INJECT] New PID detected: ${starcraftPid} (was ${lastInjectedPidRef.current}), resetting`);
          lastInjectedPidRef.current = null;
          setIsDllInjected(false);
        }

        // If already processed this PID, just update status and don't try to inject again
        if (lastInjectedPidRef.current === starcraftPid) {
          const injected = await invoke<boolean>("is_dll_injected");
          if (!isMounted) return;
          console.log(`[AUTO-INJECT] Monitoring PID ${starcraftPid}, Injected: ${injected}`);
          setIsDllInjected(injected);
          return;
        }

        // New PID - check if DLL is already injected before attempting injection
        const injected = await invoke<boolean>("is_dll_injected");
        if (!isMounted) return;
        console.log(`[AUTO-INJECT] Check result - PID: ${starcraftPid}, Injected: ${injected}`);
        setIsDllInjected(injected);

        // Try to inject if not already injected
        if (starcraftPid && !injected) {
          setIsInjecting(true);
          const currentPid = starcraftPid; // Capture PID for closure

          try {
            const dllPath = await invoke<string>("get_default_dll_path");
            const result = await invoke<string>("inject_dll", { dllPath });
            console.log("[AUTO-INJECT] Result:", result);

            setInjectStatus(result);

            // Check if injection was successful and verify
            if (result.includes("successfully") || result.includes("SUCCESS")) {
              console.log("[AUTO-INJECT] Injection succeeded, verifying DLL status...");

              // Wait a bit for DLL to be registered in process, then verify
              setTimeout(async () => {
                try {
                  const verifyInjected = await invoke<boolean>("is_dll_injected");
                  console.log("[AUTO-INJECT] Verification result:", verifyInjected);
                  setIsDllInjected(verifyInjected);

                  // Only mark as processed if verification succeeded
                  if (verifyInjected) {
                    lastInjectedPidRef.current = currentPid;
                    console.log("[AUTO-INJECT] Marked PID as processed:", currentPid);
                  }
                } catch (err) {
                  console.error("[AUTO-INJECT] Verification failed:", err);
                }
              }, 500);
            } else {
              console.log("[AUTO-INJECT] Result does not indicate success:", result);
              lastInjectedPidRef.current = currentPid; // Don't retry failed injections
            }

            setTimeout(() => setInjectStatus(""), 3000);
          } catch (error) {
            console.error("[AUTO-INJECT] Error:", error);
            setInjectStatus(`Auto-inject failed: ${error}`);
            lastInjectedPidRef.current = currentPid;
          } finally {
            setIsInjecting(false);
          }
        }
      } catch (error) {
        console.error("Auto-inject check failed:", error);
      }
    };

    checkAndInject();
    const interval = setInterval(checkAndInject, 3000); // Reduced from 1000ms to 3000ms
    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [autoInject, isInjecting, starcraftPid]);

  const addToBlacklist = async (battletag: string, memo?: string) => {
    if (!battletag.trim()) return;
    try {
      await invoke("add_to_blacklist", { battletag: battletag.trim(), memo: memo || null });
      const list = await invoke<BlacklistEntry[]>("get_blacklist");
      setBlacklist(list);
    } catch (error) {
      console.error("Failed to add to blacklist:", error);
    }
  };

  const removeFromBlacklist = async (battletag: string) => {
    try {
      await invoke("remove_from_blacklist", { battletag });
      const list = await invoke<BlacklistEntry[]>("get_blacklist");
      setBlacklist(list);
    } catch (error) {
      console.error("Failed to remove from blacklist:", error);
    }
  };

  const updateMemo = async (battletag: string, memo: string) => {
    try {
      await invoke("update_blacklist_memo", { battletag, memo });
      const list = await invoke<BlacklistEntry[]>("get_blacklist");
      setBlacklist(list);
    } catch (error) {
      console.error("Failed to update memo:", error);
    }
  };

  const handleInjectDll = async () => {
    setIsInjecting(true);
    setInjectStatus("");
    try {
      const dllPath = await invoke<string>("get_default_dll_path");
      const result = await invoke<string>("inject_dll", { dllPath });
      console.log("[INJECT] Result:", result);
      setInjectStatus(result);

      // Check if injection was successful
      if (result.includes("successfully") || result.includes("SUCCESS")) {
        console.log("[INJECT] Injection succeeded, verifying DLL status...");

        // Wait a bit for DLL to be registered in process, then verify
        setTimeout(async () => {
          const injected = await invoke<boolean>("is_dll_injected");
          console.log("[INJECT] Verification result:", injected);
          setIsDllInjected(injected);

          // Only mark as processed if verification succeeded
          if (injected && starcraftPid) {
            lastInjectedPidRef.current = starcraftPid;
            console.log("[INJECT] Marked PID as processed:", starcraftPid);
          }
        }, 500);
      } else {
        console.log("[INJECT] Result does not indicate success:", result);
        if (starcraftPid) {
          lastInjectedPidRef.current = starcraftPid; // Don't retry failed injections
        }
      }

      setTimeout(() => setInjectStatus(""), 5000);
    } catch (error) {
      console.error("[INJECT] Error:", error);
      setInjectStatus(`Error: ${error}`);
    } finally {
      setIsInjecting(false);
    }
  };

  const isInBlacklist = (battletag: string) => {
    return blacklist.some(entry => entry.battletag === battletag);
  };

  const getPingForUser = (ipAddress: string): number | null => {
    const latency = peerLatencies.find(p => p.ipAddress === ipAddress);
    return latency ? latency.avgPingMs : null;
  };

  const handleAddToBlacklist = (user: RoomUser) => {
    setSelectedUser(user);
    setDialogOpen(true);
  };

  const handleConfirmBlacklist = (battletag: string, memo: string) => {
    addToBlacklist(battletag, memo);
  };

  // History handlers
  const handleClearHistory = async () => {
    try {
      await invoke("clear_room_history");
      setRoomHistory([]);
    } catch (error) {
      console.error("Failed to clear history:", error);
    }
  };

  const handleRemoveHistoryEntry = async (timestamp: number) => {
    try {
      await invoke("remove_room_history", { timestamp });
      const history = await invoke<RoomHistoryEntry[]>("get_room_history");
      setRoomHistory(history);
    } catch (error) {
      console.error("Failed to remove history entry:", error);
    }
  };

  return (
    <div className="min-h-screen bg-background">
      {/* Header */}
      <header className="sticky top-0 z-50 border-b border-border/50 bg-background/80 backdrop-blur-xl">
        <div className="container mx-auto px-4 py-3">
          <div className="flex items-center justify-between">
            {/* Logo & Title */}
            <div className="flex items-center gap-4">
              <motion.h1
                initial={{ opacity: 0, x: -20 }}
                animate={{ opacity: 1, x: 0 }}
                className="font-display text-xl font-bold text-foreground"
              >
                <span className="text-glow-primary">SC</span>
                <span className="text-muted-foreground">:</span>
                <span className="text-accent">REMASTERED</span>
              </motion.h1>

              <ConnectionChip isConnected={isDllInjected} />
            </div>

            {/* Actions */}
            <div className="flex items-center gap-2">
              {/* StarCraft Status */}
              <Badge
                variant={starcraftPid ? "default" : "secondary"}
                className={starcraftPid ? "bg-success/20 text-success border-success/30" : ""}
              >
                <Power className="h-3 w-3 mr-1" />
                {starcraftPid ? `PID: ${starcraftPid}` : "스타 미실행"}
              </Badge>

              {/* Auto-inject Toggle */}
              <Button
                variant={autoInject ? "default" : "outline"}
                size="sm"
                onClick={() => setAutoInject(!autoInject)}
                className={autoInject ? "bg-primary/20 text-primary border-primary/30" : ""}
              >
                <Zap className="h-4 w-4 mr-1" />
                자동
              </Button>

              {/* Manual Inject */}
              <Button
                variant="outline"
                size="sm"
                onClick={handleInjectDll}
                disabled={!starcraftPid || isInjecting || isDllInjected}
              >
                {isInjecting ? "주입 중..." : "주입"}
              </Button>

              <Button
                variant="ghost"
                size="icon"
                className="text-muted-foreground hover:text-foreground"
              >
                <Settings className="h-4 w-4" />
              </Button>
            </div>
          </div>

          {/* Inject Status */}
          {injectStatus && (
            <motion.p
              initial={{ opacity: 0, y: -10 }}
              animate={{ opacity: 1, y: 0 }}
              className={`text-xs mt-2 ${
                injectStatus.includes("Error") || injectStatus.includes("failed")
                  ? "text-destructive"
                  : "text-success"
              }`}
            >
              {injectStatus}
            </motion.p>
          )}
        </div>
      </header>

      {/* Main Content */}
      <main className="container mx-auto px-4 py-6">
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Left column: Room Info + User List + History */}
          <div className="lg:col-span-2 space-y-6">
            <RoomInfoCard roomInfo={roomInfo} />
            <UserListTable
              users={roomUsers}
              getPing={getPingForUser}
              isInBlacklist={isInBlacklist}
              onAddToBlacklist={handleAddToBlacklist}
            />
            <RoomHistoryPanel
              history={roomHistory}
              onClearHistory={handleClearHistory}
              onRemoveEntry={handleRemoveHistoryEntry}
              onAddToBlacklist={addToBlacklist}
              isInBlacklist={isInBlacklist}
            />
          </div>

          {/* Right column: Blacklist */}
          <div className="lg:col-span-1">
            <BlacklistPanel
              entries={blacklist}
              onRemove={removeFromBlacklist}
              onUpdateMemo={updateMemo}
              onAddEntry={addToBlacklist}
            />
          </div>
        </div>
      </main>

      {/* Add Blacklist Dialog */}
      <AddBlacklistDialog
        user={selectedUser}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onConfirm={handleConfirmBlacklist}
      />
    </div>
  );
}

export default App;
