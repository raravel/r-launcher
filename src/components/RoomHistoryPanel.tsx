import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  History,
  ChevronDown,
  Trash2,
  Users,
  MapPin,
  Clock,
  Crown,
  AlertTriangle,
  Database,
  Ban,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";

export interface RoomHistoryEntry {
  timestamp: number;
  roomInfo: {
    inRoom: boolean;
    roomId: string;
    roomName: string;
    mapName: string;
    hostName: string;
    forceNames: string[];
  };
  users: {
    nickname: string;
    battletag: string;
    playerId: number;
    ipAddress: string;
    port: number;
  }[];
}

interface RoomHistoryPanelProps {
  history: RoomHistoryEntry[];
  onClearHistory: () => void;
  onRemoveEntry: (timestamp: number) => void;
  onAddToBlacklist: (battletag: string, memo?: string) => void;
  isInBlacklist: (battletag: string) => boolean;
}

// Format timestamp to military-style log format
const formatTimestamp = (timestamp: number): string => {
  const date = new Date(timestamp);
  const hours = date.getHours().toString().padStart(2, "0");
  const minutes = date.getMinutes().toString().padStart(2, "0");
  const seconds = date.getSeconds().toString().padStart(2, "0");
  return `${hours}:${minutes}:${seconds}`;
};

const formatDate = (timestamp: number): string => {
  const date = new Date(timestamp);
  const month = (date.getMonth() + 1).toString().padStart(2, "0");
  const day = date.getDate().toString().padStart(2, "0");
  return `${month}/${day}`;
};

// Single history entry component
const HistoryEntry = ({
  entry,
  index,
  onRemove,
  onAddToBlacklist,
  isInBlacklist,
}: {
  entry: RoomHistoryEntry;
  index: number;
  onRemove: () => void;
  onAddToBlacklist: (battletag: string, memo?: string) => void;
  isInBlacklist: (battletag: string) => boolean;
}) => {
  const [isExpanded, setIsExpanded] = useState(false);

  return (
    <motion.div
      layout
      initial={{ opacity: 0, x: -20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20, scale: 0.95 }}
      transition={{ duration: 0.3, delay: index * 0.05 }}
      className="group relative"
    >
      {/* Scan line effect overlay */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden rounded-lg opacity-0 group-hover:opacity-100 transition-opacity duration-300">
        <div
          className="absolute inset-0"
          style={{
            background:
              "repeating-linear-gradient(0deg, transparent, transparent 2px, rgba(0, 255, 255, 0.03) 2px, rgba(0, 255, 255, 0.03) 4px)",
          }}
        />
      </div>

      <div className="relative rounded-lg bg-gradient-to-br from-secondary/60 to-secondary/30 border border-border/50 hover:border-primary/40 transition-all duration-300 overflow-hidden">
        {/* Top glow line */}
        <div className="absolute top-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-primary/50 to-transparent" />

        {/* Main content */}
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          className="w-full text-left p-3 focus:outline-none focus:ring-1 focus:ring-primary/50 rounded-lg"
        >
          <div className="flex items-start gap-3">
            {/* Time indicator */}
            <div className="shrink-0 flex flex-col items-center">
              <div className="w-10 h-10 rounded-md bg-primary/10 border border-primary/30 flex flex-col items-center justify-center glow-primary">
                <span className="text-[10px] font-mono text-primary/70">
                  {formatDate(entry.timestamp)}
                </span>
                <span className="text-xs font-mono font-bold text-primary">
                  {formatTimestamp(entry.timestamp).slice(0, 5)}
                </span>
              </div>
              {/* Pulse indicator */}
              <div className="relative mt-1.5">
                <div className="w-2 h-2 rounded-full bg-primary animate-pulse" />
                <div className="absolute inset-0 w-2 h-2 rounded-full bg-primary animate-ping opacity-75" />
              </div>
            </div>

            {/* Room info */}
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-1">
                <h4 className="font-display text-sm font-semibold text-foreground truncate text-glow-primary">
                  {entry.roomInfo.roomName || "알 수 없는 방"}
                </h4>
                <ChevronDown
                  className={`h-4 w-4 text-muted-foreground transition-transform duration-300 ${
                    isExpanded ? "rotate-180" : ""
                  }`}
                />
              </div>

              {/* Room ID */}
              {entry.roomInfo.roomId && (
                <div className="mb-1">
                  <span className="font-mono text-[10px] text-muted-foreground/60">
                    ID: {entry.roomInfo.roomId}
                  </span>
                </div>
              )}

              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                <span className="flex items-center gap-1 text-muted-foreground">
                  <MapPin className="h-3 w-3 text-accent" />
                  <span className="font-mono truncate max-w-[120px]">
                    {entry.roomInfo.mapName || "-"}
                  </span>
                </span>
                <span className="flex items-center gap-1 text-muted-foreground">
                  <Crown className="h-3 w-3 text-accent" />
                  <span className="font-mono">
                    {entry.roomInfo.hostName || "-"}
                  </span>
                </span>
                <span className="flex items-center gap-1 text-primary">
                  <Users className="h-3 w-3" />
                  <span className="font-mono font-semibold">
                    {entry.users.length}명
                  </span>
                </span>
              </div>
            </div>

            {/* Delete button */}
            <Button
              variant="ghost"
              size="icon"
              onClick={(e) => {
                e.stopPropagation();
                onRemove();
              }}
              className="shrink-0 h-7 w-7 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground hover:text-destructive hover:bg-destructive/10"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </div>
        </button>

        {/* Expanded user list */}
        <AnimatePresence>
          {isExpanded && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ duration: 0.3, ease: "easeInOut" }}
              className="overflow-hidden"
            >
              <div className="px-3 pb-3 pt-1">
                <div className="border-t border-border/30 pt-3">
                  {/* Data readout header */}
                  <div className="flex items-center gap-2 mb-2">
                    <Database className="h-3 w-3 text-primary" />
                    <span className="text-[10px] font-mono text-primary uppercase tracking-wider">
                      플레이어 데이터
                    </span>
                    <div className="flex-1 h-px bg-gradient-to-r from-primary/30 to-transparent" />
                  </div>

                  {/* User list */}
                  <div className="space-y-1.5">
                    {entry.users.map((user, idx) => {
                      const isBanned = isInBlacklist(user.battletag);
                      return (
                        <motion.div
                          key={user.playerId}
                          initial={{ opacity: 0, x: -10 }}
                          animate={{ opacity: 1, x: 0 }}
                          transition={{ delay: idx * 0.03 }}
                          className={`group flex items-center gap-3 p-2 rounded border transition-colors ${
                            isBanned
                              ? "bg-destructive/10 border-destructive/30"
                              : "bg-muted/30 border-border/30 hover:border-primary/20"
                          }`}
                        >
                          <div className={`w-5 h-5 rounded border flex items-center justify-center text-[10px] font-mono font-bold ${
                            isBanned
                              ? "bg-destructive/20 border-destructive/30 text-destructive"
                              : "bg-primary/20 border-primary/30 text-primary"
                          }`}>
                            {idx + 1}
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="font-medium text-sm text-foreground truncate">
                                {user.nickname}
                              </span>
                              {isBanned && (
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-destructive/20 text-destructive font-mono">
                                  차단됨
                                </span>
                              )}
                            </div>
                            <span className="font-mono text-xs text-primary/80">
                              {user.battletag}
                            </span>
                          </div>
                          <span className="font-mono text-[10px] text-muted-foreground">
                            {user.ipAddress}
                          </span>
                          {!isBanned && (
                            <Button
                              variant="ghost"
                              size="icon"
                              onClick={(e) => {
                                e.stopPropagation();
                                onAddToBlacklist(user.battletag, `${user.nickname} - 기록에서 추가`);
                              }}
                              className="shrink-0 h-6 w-6 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground hover:text-destructive hover:bg-destructive/10"
                            >
                              <Ban className="h-3 w-3" />
                            </Button>
                          )}
                        </motion.div>
                      );
                    })}
                  </div>

                  {entry.users.length === 0 && (
                    <div className="text-center py-4 text-muted-foreground text-xs font-mono">
                      유저 데이터 없음
                    </div>
                  )}
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
};

export const RoomHistoryPanel = ({
  history,
  onClearHistory,
  onRemoveEntry,
  onAddToBlacklist,
  isInBlacklist,
}: RoomHistoryPanelProps) => {
  const [showConfirm, setShowConfirm] = useState(false);

  const handleClearAll = () => {
    if (showConfirm) {
      onClearHistory();
      setShowConfirm(false);
    } else {
      setShowConfirm(true);
      // Auto-hide confirm after 3 seconds
      setTimeout(() => setShowConfirm(false), 3000);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, delay: 0.15 }}
      className="card-cyber rounded-lg overflow-hidden"
    >
      {/* Scan lines overlay for the entire panel */}
      <div
        className="absolute inset-0 pointer-events-none opacity-30"
        style={{
          background:
            "repeating-linear-gradient(0deg, transparent, transparent 3px, rgba(0, 0, 0, 0.1) 3px, rgba(0, 0, 0, 0.1) 4px)",
        }}
      />

      {/* Header */}
      <div className="relative px-5 py-4 border-b border-border/50 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="relative">
            <div className="w-8 h-8 rounded-lg bg-accent/20 border border-accent/30 flex items-center justify-center">
              <History className="h-4 w-4 text-accent" />
            </div>
            {/* Corner accents */}
            <div className="absolute -top-0.5 -left-0.5 w-2 h-2 border-t border-l border-accent/50" />
            <div className="absolute -bottom-0.5 -right-0.5 w-2 h-2 border-b border-r border-accent/50" />
          </div>
          <div>
            <h3 className="font-display text-base font-semibold text-foreground flex items-center gap-2">
              전투 기록
              <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-primary/20 border border-primary/30 text-[10px] font-mono text-primary">
                <Clock className="h-2.5 w-2.5" />
                {history.length}
              </span>
            </h3>
            <p className="text-xs text-muted-foreground font-mono">
              이전 방 기록 · 최대 50개 저장
            </p>
          </div>
        </div>

        {/* Clear button */}
        {history.length > 0 && (
          <AnimatePresence mode="wait">
            <motion.div
              key={showConfirm ? "confirm" : "clear"}
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              transition={{ duration: 0.15 }}
            >
              <Button
                variant={showConfirm ? "destructive" : "outline"}
                size="sm"
                onClick={handleClearAll}
                className={`h-7 text-xs ${
                  showConfirm
                    ? "bg-destructive/20 border-destructive/50 text-destructive hover:bg-destructive/30"
                    : "border-border/50 text-muted-foreground hover:text-foreground"
                }`}
              >
                {showConfirm ? (
                  <>
                    <AlertTriangle className="h-3 w-3 mr-1" />
                    확인
                  </>
                ) : (
                  <>
                    <Trash2 className="h-3 w-3 mr-1" />
                    전체 삭제
                  </>
                )}
              </Button>
            </motion.div>
          </AnimatePresence>
        )}
      </div>

      {/* History list */}
      <ScrollArea className="h-[400px]">
        <div className="p-4 space-y-2">
          <AnimatePresence mode="popLayout">
            {history.map((entry, index) => (
              <HistoryEntry
                key={entry.timestamp}
                entry={entry}
                index={index}
                onRemove={() => onRemoveEntry(entry.timestamp)}
                onAddToBlacklist={onAddToBlacklist}
                isInBlacklist={isInBlacklist}
              />
            ))}
          </AnimatePresence>

          {/* Empty state */}
          {history.length === 0 && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              className="py-12 text-center"
            >
              <div className="relative inline-block">
                <History className="h-12 w-12 text-muted-foreground/20 mx-auto mb-3" />
                {/* Decorative rings */}
                <div className="absolute inset-0 rounded-full border border-muted-foreground/10 animate-ping" />
              </div>
              <p className="text-sm text-muted-foreground font-mono">
                기록된 전투가 없습니다
              </p>
              <p className="text-xs text-muted-foreground/60 mt-1">
                방을 나가면 자동으로 기록됩니다
              </p>
            </motion.div>
          )}
        </div>
      </ScrollArea>

      {/* Bottom status bar */}
      {history.length > 0 && (
        <div className="px-4 py-2 border-t border-border/30 bg-secondary/20">
          <div className="flex items-center justify-between text-[10px] font-mono text-muted-foreground">
            <span className="flex items-center gap-1.5">
              <div className="w-1.5 h-1.5 rounded-full bg-success animate-pulse" />
              데이터 동기화됨
            </span>
            <span>
              저장 용량: {history.length}/50
            </span>
          </div>
        </div>
      )}
    </motion.div>
  );
};
