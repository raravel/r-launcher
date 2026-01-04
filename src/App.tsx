import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { motion } from "framer-motion";
import { Settings, Terminal, Power, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ConnectionChip } from "@/components/ConnectionChip";
import { RoomInfoCard } from "@/components/RoomInfoCard";
import { UserListTable, type RoomUser } from "@/components/UserListTable";
import { BlacklistPanel, type BlacklistEntry } from "@/components/BlacklistPanel";
import { AddBlacklistDialog } from "@/components/AddBlacklistDialog";

interface RoomInfo {
  inRoom: boolean;
  roomName: string;
  mapName: string;
  hostName?: string;
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
  const [dllLogs, setDllLogs] = useState<string[]>([]);
  const [autoScroll, setAutoScroll] = useState(true);
  const [starcraftPid, setStarcraftPid] = useState<number | null>(null);
  const [isInjecting, setIsInjecting] = useState(false);
  const [injectStatus, setInjectStatus] = useState<string>("");
  const [autoInject, setAutoInject] = useState(true);
  const [showLogs, setShowLogs] = useState(false);
  const [selectedUser, setSelectedUser] = useState<RoomUser | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const lastInjectedPidRef = useRef<number | null>(null);
  const logsContainerRef = useRef<HTMLDivElement>(null);

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

  // Fetch logs (only when logs panel is visible)
  useEffect(() => {
    if (!showLogs) return;

    let isMounted = true;
    const fetchLogs = async () => {
      if (!isMounted) return;
      try {
        const logs = await invoke<string[]>("get_dll_logs", { lastLines: 200 });
        if (isMounted) setDllLogs(logs);
      } catch (error) {
        console.error("Failed to fetch logs:", error);
      }
    };

    fetchLogs();
    const interval = setInterval(fetchLogs, 1000); // Reduced from 500ms to 1000ms
    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [showLogs]);

  // Auto-scroll logs
  useEffect(() => {
    if (autoScroll && logsContainerRef.current) {
      logsContainerRef.current.scrollTop = logsContainerRef.current.scrollHeight;
    }
  }, [dllLogs, autoScroll]);

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

  const clearLogs = async () => {
    try {
      await invoke("clear_dll_logs");
      setDllLogs([]);
    } catch (error) {
      console.error("Failed to clear logs:", error);
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

  const getLogLineClass = (line: string) => {
    if (line.includes("ERROR") || line.includes("Failed")) {
      return "text-red-400";
    }
    if (line.includes("KICK") || line.includes("BAN") || line.includes("AUTO-BAN")) {
      return "text-yellow-400 font-bold";
    }
    if (line.includes("SESSION") || line.includes("CAPTURED") || line.includes("ROOM INFO")) {
      return "text-green-400";
    }
    if (line.includes("[WSASENDTO]") || line.includes("[SEND]")) {
      return "text-blue-400";
    }
    if (line.includes("[WSARECVFROM]") || line.includes("[RECV]")) {
      return "text-cyan-400";
    }
    if (line.includes("hook installed")) {
      return "text-green-500";
    }
    return "text-gray-300";
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

              {/* Toggle Logs */}
              <Button
                variant="ghost"
                size="icon"
                onClick={() => setShowLogs(!showLogs)}
                className={showLogs ? "text-primary" : "text-muted-foreground hover:text-foreground"}
              >
                <Terminal className="h-4 w-4" />
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
          {/* Left column: Room Info + User List */}
          <div className="lg:col-span-2 space-y-6">
            <RoomInfoCard roomInfo={roomInfo} />
            <UserListTable
              users={roomUsers}
              getPing={getPingForUser}
              isInBlacklist={isInBlacklist}
              onAddToBlacklist={handleAddToBlacklist}
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

        {/* DLL Logs (Collapsible) */}
        {showLogs && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            className="mt-6 card-cyber rounded-lg overflow-hidden"
          >
            <div className="px-5 py-4 border-b border-border/50 flex items-center justify-between">
              <div className="flex items-center gap-3">
                <div className="w-8 h-8 rounded-lg bg-primary/20 border border-primary/30 flex items-center justify-center">
                  <Terminal className="h-4 w-4 text-primary" />
                </div>
                <div>
                  <h3 className="font-display text-base font-semibold text-foreground">
                    DLL 로그
                  </h3>
                  <p className="text-xs text-muted-foreground">
                    네트워크 후킹 활동 ({dllLogs.length} 줄)
                  </p>
                </div>
              </div>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setAutoScroll(!autoScroll)}
                  className={autoScroll ? "bg-primary/10 border-primary/30" : ""}
                >
                  {autoScroll ? "자동 스크롤: ON" : "자동 스크롤: OFF"}
                </Button>
                <Button variant="outline" size="sm" onClick={clearLogs}>
                  지우기
                </Button>
              </div>
            </div>
            <div
              ref={logsContainerRef}
              className="h-[300px] overflow-y-auto bg-black/50 p-4"
            >
              <div className="font-mono text-xs space-y-0.5">
                {dllLogs.length === 0 ? (
                  <p className="text-muted-foreground">
                    로그가 없습니다. DLL을 주입하면 네트워크 활동이 표시됩니다.
                  </p>
                ) : (
                  dllLogs.map((line, index) => (
                    <div key={index} className={getLogLineClass(line)}>
                      {line}
                    </div>
                  ))
                )}
              </div>
            </div>
          </motion.div>
        )}
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
