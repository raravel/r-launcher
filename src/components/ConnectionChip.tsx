import { motion } from "framer-motion";
import { Wifi, WifiOff } from "lucide-react";

interface ConnectionChipProps {
  isConnected: boolean;
  className?: string;
}

export const ConnectionChip = ({ isConnected, className = "" }: ConnectionChipProps) => {
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      className={`relative inline-flex items-center gap-2 px-4 py-2 rounded-full border ${className} ${
        isConnected
          ? "border-success/50 bg-success/10"
          : "border-destructive/50 bg-destructive/10"
      }`}
    >
      {/* Pulse ring animation for connected state */}
      {isConnected && (
        <span className="absolute inset-0 rounded-full animate-pulse-ring border border-success/30" />
      )}

      {/* Status indicator dot */}
      <span className="relative flex h-2.5 w-2.5">
        {isConnected && (
          <span className="absolute inline-flex h-full w-full rounded-full bg-success opacity-75 animate-ping" />
        )}
        <span
          className={`relative inline-flex h-2.5 w-2.5 rounded-full ${
            isConnected ? "bg-success" : "bg-destructive"
          }`}
        />
      </span>

      {/* Icon */}
      {isConnected ? (
        <Wifi className="h-4 w-4 text-success" />
      ) : (
        <WifiOff className="h-4 w-4 text-destructive" />
      )}

      {/* Text */}
      <span className={`font-mono text-sm font-medium ${
        isConnected ? "text-success" : "text-destructive"
      }`}>
        {isConnected ? "연결됨" : "연결 안됨"}
      </span>
    </motion.div>
  );
};
