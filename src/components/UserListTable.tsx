import { motion } from "framer-motion";
import { Ban, Signal, SignalLow, SignalMedium, SignalHigh, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";

export interface RoomUser {
  nickname: string;
  battletag: string;
  playerId: number;
  ipAddress: string;
  port: number;
}

interface UserListTableProps {
  users: RoomUser[];
  getPing: (ipAddress: string) => number | null;
  isInBlacklist: (battletag: string) => boolean;
  onAddToBlacklist: (user: RoomUser) => void;
}

const getPingColor = (ping: number) => {
  if (ping < 50) return "text-success";
  if (ping < 100) return "text-accent";
  if (ping < 150) return "text-warning";
  return "text-destructive";
};

const PingIndicator = ({ ping }: { ping: number | null }) => {
  if (ping === null) {
    return <span className="text-muted-foreground">-</span>;
  }

  const color = getPingColor(ping);

  if (ping < 50) return <SignalHigh className={`h-4 w-4 ${color}`} />;
  if (ping < 100) return <SignalMedium className={`h-4 w-4 ${color}`} />;
  if (ping < 150) return <SignalLow className={`h-4 w-4 ${color}`} />;
  return <Signal className={`h-4 w-4 ${color}`} />;
};

export const UserListTable = ({
  users,
  getPing,
  isInBlacklist,
  onAddToBlacklist,
}: UserListTableProps) => {
  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, delay: 0.1 }}
      className="card-cyber rounded-lg overflow-hidden"
    >
      {/* Header */}
      <div className="px-5 py-4 border-b border-border/50 flex items-center gap-3">
        <div className="w-8 h-8 rounded-lg bg-primary/20 border border-primary/30 flex items-center justify-center">
          <Users className="h-4 w-4 text-primary" />
        </div>
        <div>
          <h3 className="font-display text-base font-semibold text-foreground">
            유저 목록
          </h3>
          <p className="text-xs text-muted-foreground">
            현재 방에 있는 플레이어 ({users.length}명)
          </p>
        </div>
      </div>

      {/* Table */}
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow className="border-border/50 hover:bg-transparent">
              <TableHead className="text-muted-foreground font-mono text-xs">닉네임</TableHead>
              <TableHead className="text-muted-foreground font-mono text-xs">배틀태그</TableHead>
              <TableHead className="text-muted-foreground font-mono text-xs">IP</TableHead>
              <TableHead className="text-muted-foreground font-mono text-xs text-center">핑</TableHead>
              <TableHead className="text-muted-foreground font-mono text-xs text-right">액션</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {users.map((user, index) => {
              const ping = getPing(user.ipAddress);
              const isBanned = isInBlacklist(user.battletag);

              return (
                <motion.tr
                  key={user.playerId}
                  initial={{ opacity: 0, x: -20 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ duration: 0.3, delay: index * 0.05 }}
                  className="border-border/30 hover:bg-primary/5 transition-colors group"
                >
                  <TableCell className="font-medium text-foreground">
                    {user.nickname}
                  </TableCell>
                  <TableCell className="font-mono text-sm text-primary">
                    {user.battletag}
                  </TableCell>
                  <TableCell className="font-mono text-sm text-muted-foreground">
                    {user.ipAddress}
                  </TableCell>
                  <TableCell className="text-center">
                    <div className="flex items-center justify-center gap-2">
                      <PingIndicator ping={ping} />
                      {ping !== null && (
                        <span className={`font-mono text-sm ${getPingColor(ping)}`}>
                          {ping}ms
                        </span>
                      )}
                    </div>
                  </TableCell>
                  <TableCell className="text-right">
                    {isBanned ? (
                      <Badge variant="destructive" className="text-xs">
                        차단됨
                      </Badge>
                    ) : (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => onAddToBlacklist(user)}
                        className="text-destructive border-destructive/50 hover:text-destructive hover:bg-destructive/10"
                      >
                        <Ban className="h-4 w-4 mr-1" />
                        차단
                      </Button>
                    )}
                  </TableCell>
                </motion.tr>
              );
            })}
          </TableBody>
        </Table>
      </div>

      {users.length === 0 && (
        <div className="py-12 text-center text-muted-foreground">
          <Users className="h-12 w-12 text-muted-foreground/30 mx-auto mb-3" />
          <p className="font-mono text-sm">방에 유저가 없습니다</p>
        </div>
      )}
    </motion.div>
  );
};
