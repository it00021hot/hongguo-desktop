#!/system/bin/sh
# 红果官方 App 抓包环境一键应用（模拟器每次重启后重跑一次即可）：
#   adb push captures/hg-capture-apply.sh /data/local/tmp/hg-capture-apply.sh
#   adb shell sh /data/local/tmp/hg-capture-apply.sh
# 做四件事：合成 CA 目录 → bind-mount 盖住两个信任库 → 封 QUIC。
#
# ⚠️ 四个必须踩准的点（2026-10-25 抓包复现实录，错了就是「客户端不信任
#    代理证书」，mitmproxy 侧只看到 tls alert certificate unknown）：
# 1. **信任库真实路径是版本化 APEX**：`/apex/com.android.conscrypt@<版本>/cacerts`
#    （未版本化的 `/apex/com.android.conscrypt/cacerts` 在本机镜像上是空壳目录，
#    且挂上去会失败）。脚本按通配符取真实目录，不写死版本号。
# 2. **CA 文件权限必须是 644 + root:root**：从 /sdcard 复制来的证书带
#    media_rw 的 660 属主，App 进程（untrusted_app）读不到 →
#    日志里是 `DirectoryCertificateSrc: ... open failed: EACCES`。
# 3. **SELinux 标签要显式 chcon 成 system_security_cacerts_file**：
#    拷进 /data/local/tmp 的默认标签是 shell_data_file，App 同样读不到。
# 4. **重跑前必须先 umount**：bind mount 生效后，APEX 目录里看到的是上一轮
#    的 STAGE（模拟器的 sdc3 覆盖层被盖住），直接 cp 会把「被盖住后的内容」
#    当系统证书拷进 STAGE —— 结果 STAGE 只剩 mitm CA 一张，系统证书全丢，
#    App 反而连普通 HTTPS 都握手失败。脚本因此先 umount 再拷，保证幂等。
set -x

CERT=/sdcard/c8750f0d.0
APEX_CERTS=$(ls -d /apex/com.android.conscrypt@*/cacerts 2>/dev/null | head -1)
STAGE=/data/local/tmp/hg-cacerts

# 0) 卸掉上一轮 bind mount，让真实系统证书目录重新可见
#    zygote 命名空间里也要卸：上一轮可能把 STAGE 挂进了 zygote（见 2.5）
APEX_UNVERSIONED=/apex/com.android.conscrypt/cacerts
for z in $(pidof zygote64) $(pidof zygote); do
  nsenter -t "$z" -m -- umount "$APEX_CERTS" 2>/dev/null
  nsenter -t "$z" -m -- umount "$APEX_UNVERSIONED" 2>/dev/null
  nsenter -t "$z" -m -- umount /system/etc/security/cacerts 2>/dev/null
done
umount "$APEX_CERTS" 2>/dev/null
umount /system/etc/security/cacerts 2>/dev/null
umount /apex/com.android.conscrypt/cacerts 2>/dev/null

# 1) 合成证书目录 = APEX 现有全部证书 + mitmproxy CA
rm -rf "$STAGE"
mkdir -p "$STAGE"
if [ -n "$APEX_CERTS" ]; then
  cp "$APEX_CERTS"/* "$STAGE"/ 2>/dev/null
fi
if [ -z "$(ls -A "$STAGE")" ]; then
  # APEX 目录为空（镜像没带系统证书）→ 退回 /system 旧信任库
  cp /system/etc/security/cacerts/* "$STAGE"/ 2>/dev/null
fi
cp "$CERT" "$STAGE"/
chown root:root "$STAGE"/*
chmod 644 "$STAGE"/*
chcon -R u:object_r:system_security_cacerts_file:s0 "$STAGE"

# 2) bind-mount 盖住三个信任库：APEX 版本化路径、APEX 未版本化路径、/system 旧路径兜底。
#    未版本化路径必须盖：2026-10-26 新 MuMu 镜像实测，/apex/com.android.conscrypt 与
#    版本化路径是两条独立 bind 链（不同 inode），Conscrypt 默认读未版本化路径——只盖
#    版本化路径时 App 侧报 Trust anchor not found。旧镜像该路径是空壳目录可跳过。
APEX_UNVERSIONED=/apex/com.android.conscrypt/cacerts
mount -o bind "$STAGE" "$APEX_CERTS"
if [ -d "$APEX_UNVERSIONED" ]; then
  mount -o bind "$STAGE" "$APEX_UNVERSIONED" 2>/dev/null || echo "WARN: mount $APEX_UNVERSIONED failed (旧镜像空壳可忽略)"
fi
mount -o bind "$STAGE" /system/etc/security/cacerts

# 2.5) 关键：/apex 是私有挂载树，root shell 里做的 bind mount 不会传播进 App 的
#      mount namespace（/system 那条会）。Android 15 上 Conscrypt 读 APEX，
#      只挂在 shell 命名空间时 App 侧仍是 `tls alert certificate unknown`。
#      把同样的 bind 再做进 zygote 的命名空间，App 新进程才会继承。
#      做完必须重启 App（force-stop 后冷启）：已在跑的进程看不到新挂载。
for z in $(pidof zygote64) $(pidof zygote); do
  nsenter -t "$z" -m -- mount -o bind "$STAGE" "$APEX_CERTS" 2>/dev/null
  if [ -d "$APEX_UNVERSIONED" ]; then
    nsenter -t "$z" -m -- mount -o bind "$STAGE" "$APEX_UNVERSIONED" 2>/dev/null
  fi
  nsenter -t "$z" -m -- mount -o bind "$STAGE" /system/etc/security/cacerts 2>/dev/null
done

# 3) 封 QUIC：UDP 443 过不了 HTTP 代理，REJECT 逼 TTNet 回落 TCP
iptables -I OUTPUT -p udp --dport 443 -j REJECT 2>/dev/null
ip6tables -I OUTPUT -p udp --dport 443 -j REJECT 2>/dev/null

echo APPLY-DONE
echo "APEX_CERTS=$APEX_CERTS"
echo "next: adb shell am force-stop com.phoenix.read（App 必须冷启才会读到新证书）"
ls "$APEX_CERTS" | wc -l
ls -lZ "$APEX_CERTS/c8750f0d.0"
