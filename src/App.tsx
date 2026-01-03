import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";

interface RoomUser {
  nickname: string;
  battletag: string;
  playerId: number;
  ipAddress: string;
  port: number;
}

interface BlacklistEntry {
  battletag: string;
  memo: string;
}

interface RoomInfo {
  inRoom: boolean;
  roomName: string;
  mapName: string;
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
  const [newBattletag, setNewBattletag] = useState("");
  const [newMemo, setNewMemo] = useState("");
  const [isConnected, setIsConnected] = useState(false);
  const [isDllInjected, setIsDllInjected] = useState(false);
  const [dllLogs, setDllLogs] = useState<string[]>([]);
  const [autoScroll, setAutoScroll] = useState(true);
  const [starcraftPid, setStarcraftPid] = useState<number | null>(null);
  const [isInjecting, setIsInjecting] = useState(false);
  const [injectStatus, setInjectStatus] = useState<string>("");
  const [autoInject, setAutoInject] = useState(true);
  const lastInjectedPidRef = useRef<number | null>(null);
  const logsContainerRef = useRef<HTMLDivElement>(null);

  // Fetch data on mount and periodically
  useEffect(() => {
    const fetchData = async () => {
      try {
        const connected = await invoke<boolean>("is_connected");
        setIsConnected(connected);

        const injected = await invoke<boolean>("is_dll_injected");
        setIsDllInjected(injected);

        const users = await invoke<RoomUser[]>("get_room_users");
        setRoomUsers(users);

        const list = await invoke<BlacklistEntry[]>("get_blacklist");
        setBlacklist(list);

        // Fetch room info
        const info = await invoke<RoomInfo>("get_room_info");
        setRoomInfo(info);

        // Fetch peer latencies
        const latencies = await invoke<PeerLatency[]>("get_peer_latencies");
        setPeerLatencies(latencies);

        // Check for StarCraft process
        const pid = await invoke<number | null>("find_starcraft_process");
        setStarcraftPid(pid);
      } catch (error) {
        console.error("Failed to fetch data:", error);
      }
    };

    fetchData();
    const interval = setInterval(fetchData, 1000);
    return () => clearInterval(interval);
  }, []);

  // Fetch logs more frequently
  useEffect(() => {
    const fetchLogs = async () => {
      try {
        const logs = await invoke<string[]>("get_dll_logs", { lastLines: 200 });
        setDllLogs(logs);
      } catch (error) {
        console.error("Failed to fetch logs:", error);
      }
    };

    fetchLogs();
    const interval = setInterval(fetchLogs, 500);
    return () => clearInterval(interval);
  }, []);

  // Auto-scroll logs (only within the log container)
  useEffect(() => {
    if (autoScroll && logsContainerRef.current) {
      logsContainerRef.current.scrollTop = logsContainerRef.current.scrollHeight;
    }
  }, [dllLogs, autoScroll]);

  // Auto-inject DLL every 1 second
  useEffect(() => {
    if (!autoInject) return;

    const checkAndInject = async () => {
      try {
        // Check for StarCraft process
        const pid = await invoke<number | null>("find_starcraft_process");
        setStarcraftPid(pid);

        if (!pid) {
          // Process not running, reset injection state
          lastInjectedPidRef.current = null;
          setIsDllInjected(false);
          return;
        }

        // If PID changed (new process), reset injection state
        if (lastInjectedPidRef.current !== null && lastInjectedPidRef.current !== pid) {
          lastInjectedPidRef.current = null;
          setIsDllInjected(false);
        }

        // Check if DLL is already injected
        const injected = await invoke<boolean>("is_dll_injected");
        setIsDllInjected(injected);

        // If not injected and not currently injecting, auto-inject
        if (pid && !injected && !isInjecting && lastInjectedPidRef.current !== pid) {
          setIsInjecting(true);
          try {
            const dllPath = await invoke<string>("get_default_dll_path");
            const result = await invoke<string>("inject_dll", { dllPath });
            setInjectStatus(result);
            lastInjectedPidRef.current = pid;
            setTimeout(() => setInjectStatus(""), 3000);
          } catch (error) {
            setInjectStatus(`Auto-inject failed: ${error}`);
            // Don't retry immediately on failure for this PID
            lastInjectedPidRef.current = pid;
          } finally {
            setIsInjecting(false);
          }
        }
      } catch (error) {
        console.error("Auto-inject check failed:", error);
      }
    };

    checkAndInject();
    const interval = setInterval(checkAndInject, 1000);
    return () => clearInterval(interval);
  }, [autoInject, isInjecting]);

  const addToBlacklist = async (battletag: string, memo?: string) => {
    if (!battletag.trim()) return;
    try {
      await invoke("add_to_blacklist", { battletag: battletag.trim(), memo: memo || null });
      const list = await invoke<BlacklistEntry[]>("get_blacklist");
      setBlacklist(list);
      setNewBattletag("");
      setNewMemo("");
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
      setInjectStatus(result);
      // Clear status after 5 seconds
      setTimeout(() => setInjectStatus(""), 5000);
    } catch (error) {
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

  // Colorize log lines
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
    <main className="min-h-screen bg-background p-4">
      <div className="mx-auto max-w-6xl space-y-4">
        {/* Header */}
        <div className="flex items-center justify-between">
          <h1 className="text-2xl font-bold">R-Launcher</h1>
          <div className="flex gap-2">
            <Badge variant={isDllInjected ? "default" : "destructive"}>
              DLL: {isDllInjected ? "Injected" : "Not Injected"}
            </Badge>
            <Badge variant={isConnected ? "default" : "secondary"}>
              Memory: {isConnected ? "Connected" : "Disconnected"}
            </Badge>
          </div>
        </div>

        <Separator />

        <div className="grid gap-4 lg:grid-cols-3">
          {/* Room Users Card */}
          <Card>
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between">
                <div>
                  <CardTitle className="text-lg">Room Users</CardTitle>
                  <CardDescription>
                    Current users ({roomUsers.length})
                  </CardDescription>
                </div>
                {roomInfo.inRoom && (
                  <Badge variant="outline" className="text-xs">
                    In Room
                  </Badge>
                )}
              </div>
              {roomInfo.inRoom && (roomInfo.roomName || roomInfo.mapName) && (
                <div className="mt-2 space-y-1 text-xs">
                  {roomInfo.roomName && (
                    <div className="flex gap-2">
                      <span className="text-muted-foreground">Room:</span>
                      <span className="font-medium truncate">{roomInfo.roomName}</span>
                    </div>
                  )}
                  {roomInfo.mapName && (
                    <div className="flex gap-2">
                      <span className="text-muted-foreground">Map:</span>
                      <span className="font-medium truncate">{roomInfo.mapName}</span>
                    </div>
                  )}
                </div>
              )}
            </CardHeader>
            <CardContent>
              <ScrollArea className="h-[200px]">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Nickname</TableHead>
                      <TableHead>Battletag</TableHead>
                      <TableHead>IP</TableHead>
                      <TableHead>Ping</TableHead>
                      <TableHead className="w-[50px]">Action</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {roomUsers.length === 0 ? (
                      <TableRow>
                        <TableCell
                          colSpan={5}
                          className="text-center text-muted-foreground"
                        >
                          No users in room
                        </TableCell>
                      </TableRow>
                    ) : (
                      roomUsers.map((user) => (
                        <TableRow key={user.playerId}>
                          <TableCell className="font-medium text-sm">
                            {user.nickname}
                          </TableCell>
                          <TableCell className="text-xs text-muted-foreground">
                            {user.battletag}
                          </TableCell>
                          <TableCell className="text-xs text-muted-foreground font-mono">
                            {user.ipAddress}
                          </TableCell>
                          <TableCell className="text-xs font-mono">
                            {(() => {
                              const ping = getPingForUser(user.ipAddress);
                              if (ping === null) return <span className="text-muted-foreground">-</span>;
                              const color = ping < 50 ? "text-green-500" : ping < 100 ? "text-yellow-500" : "text-red-500";
                              return <span className={color}>{ping}ms</span>;
                            })()}
                          </TableCell>
                          <TableCell>
                            {isInBlacklist(user.battletag) ? (
                              <Badge variant="destructive" className="text-xs">
                                Banned
                              </Badge>
                            ) : (
                              <Button
                                variant="outline"
                                size="sm"
                                className="h-6 text-xs"
                                onClick={() => addToBlacklist(user.battletag)}
                              >
                                Ban
                              </Button>
                            )}
                          </TableCell>
                        </TableRow>
                      ))
                    )}
                  </TableBody>
                </Table>
              </ScrollArea>
            </CardContent>
          </Card>

          {/* Blacklist Card */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-lg">Blacklist</CardTitle>
              <CardDescription>
                Auto-ban targets ({blacklist.length}/10)
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {/* Add new battletag */}
              <div className="space-y-2">
                <div className="flex gap-2">
                  <Input
                    placeholder="Battletag#1234"
                    value={newBattletag}
                    onChange={(e) => setNewBattletag(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        addToBlacklist(newBattletag, newMemo);
                      }
                    }}
                    className="h-8 text-sm"
                  />
                  <Button
                    onClick={() => addToBlacklist(newBattletag, newMemo)}
                    disabled={!newBattletag.trim() || blacklist.length >= 10}
                    size="sm"
                    className="h-8"
                  >
                    Add
                  </Button>
                </div>
                <Input
                  placeholder="Memo (optional)"
                  value={newMemo}
                  onChange={(e) => setNewMemo(e.target.value)}
                  className="h-7 text-xs"
                />
              </div>

              <Separator />

              {/* Blacklist items */}
              <ScrollArea className="h-[140px]">
                <div className="space-y-2">
                  {blacklist.length === 0 ? (
                    <p className="text-center text-sm text-muted-foreground py-4">
                      No targets in blacklist
                    </p>
                  ) : (
                    blacklist.map((entry, index) => (
                      <div
                        key={entry.battletag}
                        className="rounded-md border p-2 space-y-1"
                      >
                        <div className="flex items-center justify-between">
                          <span className="text-sm">
                            <span className="text-muted-foreground mr-2">
                              {index + 1}.
                            </span>
                            {entry.battletag}
                          </span>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => removeFromBlacklist(entry.battletag)}
                            className="h-5 w-5 p-0 text-destructive hover:text-destructive"
                          >
                            ×
                          </Button>
                        </div>
                        <Input
                          placeholder="Add memo..."
                          defaultValue={entry.memo}
                          onBlur={(e) => {
                            if (e.target.value !== entry.memo) {
                              updateMemo(entry.battletag, e.target.value);
                            }
                          }}
                          className="h-6 text-xs bg-muted/50"
                        />
                      </div>
                    ))
                  )}
                </div>
              </ScrollArea>
            </CardContent>
          </Card>

          {/* Status Card */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-lg">Status</CardTitle>
              <CardDescription>System information</CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="space-y-2 text-sm">
                <div className="flex justify-between">
                  <span className="text-muted-foreground">StarCraft:</span>
                  <span className={starcraftPid ? "text-green-500" : "text-red-500"}>
                    {starcraftPid ? `Running (PID: ${starcraftPid})` : "Not Running"}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">DLL Hooks:</span>
                  <span className={isDllInjected ? "text-green-500" : "text-red-500"}>
                    {isDllInjected ? "Installed" : "Not Installed"}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Shared Memory:</span>
                  <span className={isConnected ? "text-green-500" : "text-yellow-500"}>
                    {isConnected ? "Connected" : "Waiting..."}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Log lines:</span>
                  <span>{dllLogs.length}</span>
                </div>
              </div>

              <Separator />

              {/* Auto-inject toggle */}
              <div className="flex items-center justify-between">
                <span className="text-sm text-muted-foreground">Auto-inject:</span>
                <Button
                  variant={autoInject ? "default" : "outline"}
                  size="sm"
                  onClick={() => setAutoInject(!autoInject)}
                  className="h-7"
                >
                  {autoInject ? "ON" : "OFF"}
                </Button>
              </div>

              {/* Inject DLL Button */}
              <Button
                onClick={handleInjectDll}
                disabled={!starcraftPid || isInjecting || isDllInjected}
                className="w-full"
                variant={isDllInjected ? "secondary" : "default"}
              >
                {isInjecting
                  ? "Injecting..."
                  : isDllInjected
                  ? "DLL Injected"
                  : starcraftPid
                  ? "Inject DLL"
                  : "Start StarCraft First"}
              </Button>

              {injectStatus && (
                <p className={`text-xs ${injectStatus.includes("Error") || injectStatus.includes("failed") ? "text-red-400" : "text-green-400"}`}>
                  {injectStatus}
                </p>
              )}
            </CardContent>
          </Card>
        </div>

        {/* DLL Logs */}
        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <div>
                <CardTitle className="text-lg">DLL Logs</CardTitle>
                <CardDescription>
                  Network hook activity (recv, send, wsasendto, wsarecvfrom)
                </CardDescription>
              </div>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setAutoScroll(!autoScroll)}
                  className={autoScroll ? "bg-primary/10" : ""}
                >
                  {autoScroll ? "Auto-scroll: ON" : "Auto-scroll: OFF"}
                </Button>
                <Button variant="outline" size="sm" onClick={clearLogs}>
                  Clear
                </Button>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            <div
              ref={logsContainerRef}
              className="h-[300px] overflow-y-auto rounded-md border bg-black/90 p-3"
            >
              <div className="font-mono text-xs space-y-0.5">
                {dllLogs.length === 0 ? (
                  <p className="text-muted-foreground">
                    No logs yet. Inject DLL into StarCraft to see network activity.
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
          </CardContent>
        </Card>
      </div>
    </main>
  );
}

export default App;
