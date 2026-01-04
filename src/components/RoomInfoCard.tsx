import { motion } from "framer-motion";
import { Gamepad2, MapPin, Crown, Home } from "lucide-react";

interface RoomInfo {
  inRoom: boolean;
  roomName: string;
  mapName: string;
  hostName?: string;
}

interface RoomInfoCardProps {
  roomInfo: RoomInfo;
}

export const RoomInfoCard = ({ roomInfo }: RoomInfoCardProps) => {
  if (!roomInfo.inRoom) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.4 }}
        className="card-cyber rounded-lg p-5"
      >
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-muted/50 border border-border/50 flex items-center justify-center">
            <Home className="h-5 w-5 text-muted-foreground" />
          </div>
          <div>
            <h2 className="font-display text-lg font-bold text-muted-foreground">
              방에 입장하지 않음
            </h2>
            <p className="text-xs text-muted-foreground font-mono">
              게임 방에 입장하면 정보가 표시됩니다
            </p>
          </div>
        </div>
      </motion.div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="card-cyber rounded-lg p-5"
    >
      {/* Header */}
      <div className="flex items-center gap-3 mb-4">
        <div className="w-10 h-10 rounded-lg bg-primary/20 border border-primary/30 flex items-center justify-center glow-primary">
          <Gamepad2 className="h-5 w-5 text-primary" />
        </div>
        <div className="min-w-0 flex-1">
          <h2 className="font-display text-lg font-bold text-foreground text-glow-primary truncate">
            {roomInfo.roomName || "알 수 없는 방"}
          </h2>
          <p className="text-xs text-muted-foreground font-mono">
            현재 방 정보
          </p>
        </div>
      </div>

      {/* Info grid */}
      <div className="grid grid-cols-2 gap-3">
        <div className="flex items-center gap-2 p-3 rounded-md bg-secondary/50 border border-border/50">
          <MapPin className="h-4 w-4 text-accent shrink-0" />
          <div className="min-w-0">
            <p className="text-xs text-muted-foreground">맵</p>
            <p className="font-mono text-sm text-foreground truncate">
              {roomInfo.mapName || "-"}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2 p-3 rounded-md bg-secondary/50 border border-border/50">
          <Crown className="h-4 w-4 text-accent shrink-0" />
          <div className="min-w-0">
            <p className="text-xs text-muted-foreground">방장</p>
            <p className="font-mono text-sm text-foreground truncate">
              {roomInfo.hostName || "-"}
            </p>
          </div>
        </div>
      </div>
    </motion.div>
  );
};
