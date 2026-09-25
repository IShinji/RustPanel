import { IconButton, Input, NumberInput, SelectRow, StatusPill, ToggleRow } from "../components/form-controls";
import { FirewallAction, FirewallBackend, FirewallDirection, FirewallProtocol, FirewallRule, SshKeyAlgorithm, SshKeyItem, SshLoginEvent, WafAttackEvent, WafRule, WafRuleKind } from "../gen/rustpanel/v1/security_pb";
import { safeError } from "../lib/format";
import { defaultFirewallForm, defaultSecurityOptions, defaultSshKeyForm, defaultSshSettings, defaultWafRuleForm, defaultWafSettings, type FirewallForm, type SecurityOptionsForm, type SshKeyForm, type SshSettingsForm, type WafRuleForm, type WafSettingsForm } from "../lib/forms";
import { firewallActionLabel, firewallDirectionLabel, firewallProtocolLabel, sshAlgorithmLabel, wafKindLabel } from "../lib/labels";
import { type Clients } from "../lib/rpc";
import { Ban, Copy, FileDown, FileText, FileUp, Globe, Plus, Power, RefreshCw, Save, Shield, ShieldAlert, ShieldCheck, TerminalSquare } from "lucide-react";
import { useState } from "react";
import { useMountEffect } from "../lib/hooks";

export function SecurityPanel({ clients }: { clients: Clients }) {
  const [rules, setRules] = useState<FirewallRule[]>([]);
  const [ruleForm, setRuleForm] = useState<FirewallForm>(defaultFirewallForm);
  const [options, setOptions] = useState<SecurityOptionsForm>(defaultSecurityOptions);
  const [wafSettings, setWafSettings] = useState<WafSettingsForm>(defaultWafSettings);
  const [wafRules, setWafRules] = useState<WafRule[]>([]);
  const [wafRuleForm, setWafRuleForm] = useState<WafRuleForm>(defaultWafRuleForm);
  const [wafEvents, setWafEvents] = useState<WafAttackEvent[]>([]);
  const [sshSettings, setSshSettings] = useState<SshSettingsForm>(defaultSshSettings);
  const [sshKeys, setSshKeys] = useState<SshKeyItem[]>([]);
  const [sshKeyForm, setSshKeyForm] = useState<SshKeyForm>(defaultSshKeyForm);
  const [sshEvents, setSshEvents] = useState<SshLoginEvent[]>([]);
  const [backupJson, setBackupJson] = useState("");
  const [status, setStatus] = useState("");

  const load = async () => {
    try {
      const [firewallResponse, wafResponse, wafEventResponse, sshResponse, sshEventResponse] = await Promise.all([
        clients.security.listFirewallRules({}),
        clients.security.getWafSettings({}),
        clients.security.listWafAttackEvents({ limit: 100 }),
        clients.security.getSshSettings({}),
        clients.security.listSshLoginEvents({ limit: 100 })
      ]);
      setRules(firewallResponse.rules);
      if (firewallResponse.options) {
        setOptions({ ...defaultSecurityOptions, ...firewallResponse.options });
      }
      if (wafResponse.settings) {
        setWafSettings({ ...defaultWafSettings, ...wafResponse.settings });
      }
      setWafRules(wafResponse.rules);
      setWafEvents(wafEventResponse.events);
      if (sshResponse.settings) {
        setSshSettings({ ...defaultSshSettings, ...sshResponse.settings });
      }
      setSshKeys(sshResponse.keys);
      setSshEvents(sshEventResponse.events);
      setStatus("");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  useMountEffect(() => load());

  const saveRule = async () => {
    try {
      const isIcmp = ruleForm.protocol === FirewallProtocol.ICMP;
      await clients.security.upsertFirewallRule({
        rule: {
          id: ruleForm.id,
          name: ruleForm.name,
          protocol: ruleForm.protocol,
          action: ruleForm.action,
          direction: ruleForm.direction,
          portStart: isIcmp ? 0 : Number(ruleForm.portStart || 0),
          portEnd: isIcmp || !ruleForm.portEnd ? 0 : Number(ruleForm.portEnd),
          source: ruleForm.source,
          destination: ruleForm.destination,
          enabled: ruleForm.enabled,
          comment: ruleForm.comment,
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      setRuleForm(defaultFirewallForm);
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const editRule = (rule: FirewallRule) => {
    setRuleForm({
      id: rule.id,
      name: rule.name,
      protocol: rule.protocol,
      action: rule.action,
      direction: rule.direction,
      portStart: rule.portStart ? String(rule.portStart) : "",
      portEnd: rule.portEnd ? String(rule.portEnd) : "",
      source: rule.source,
      destination: rule.destination,
      enabled: rule.enabled,
      comment: rule.comment
    });
  };

  const deleteRule = async (rule: FirewallRule) => {
    try {
      await clients.security.deleteFirewallRule({ id: rule.id });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const toggleRule = async (rule: FirewallRule) => {
    try {
      await clients.security.setFirewallRuleEnabled({ id: rule.id, enabled: !rule.enabled });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveOptions = async () => {
    try {
      const response = await clients.security.updateSecurityOptions({ options });
      if (response.options) {
        setOptions({ ...defaultSecurityOptions, ...response.options });
      }
      setStatus(response.options?.lastApplyMessage ?? "安全选项已保存");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const exportRules = async () => {
    try {
      const response = await clients.security.exportFirewallRules({});
      setBackupJson(response.backupJson);
      setStatus("规则备份已生成");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const importRules = async () => {
    try {
      const response = await clients.security.importFirewallRules({
        backupJson,
        replaceExisting: true
      });
      setRules(response.rules);
      if (response.options) {
        setOptions({ ...defaultSecurityOptions, ...response.options });
      }
      setStatus("规则备份已导入");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveWafSettings = async () => {
    try {
      const response = await clients.security.updateWafSettings({ settings: wafSettings });
      if (response.settings) {
        setWafSettings({ ...defaultWafSettings, ...response.settings });
        setStatus(response.settings.lastApplyMessage || "WAF 配置已保存");
      }
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveWafRule = async () => {
    try {
      await clients.security.upsertWafRule({
        rule: {
          id: wafRuleForm.id,
          name: wafRuleForm.name,
          kind: wafRuleForm.kind,
          pattern: wafRuleForm.pattern,
          enabled: wafRuleForm.enabled,
          scopeDomain: wafRuleForm.scopeDomain,
          comment: wafRuleForm.comment,
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      setWafRuleForm(defaultWafRuleForm);
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const editWafRule = (rule: WafRule) => {
    setWafRuleForm({
      id: rule.id,
      name: rule.name,
      kind: rule.kind,
      pattern: rule.pattern,
      enabled: rule.enabled,
      scopeDomain: rule.scopeDomain,
      comment: rule.comment
    });
  };

  const deleteWafRule = async (rule: WafRule) => {
    try {
      await clients.security.deleteWafRule({ id: rule.id });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveSshSettings = async () => {
    try {
      const response = await clients.security.updateSshSettings({ settings: sshSettings });
      if (response.settings) {
        setSshSettings({ ...defaultSshSettings, ...response.settings });
        setStatus(response.settings.lastApplyMessage || "SSH 配置已保存");
      }
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const generateSshKey = async () => {
    try {
      await clients.security.generateSshKey({
        name: sshKeyForm.name,
        algorithm: sshKeyForm.algorithm
      });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const wafIpRanking = wafEvents.reduce<Array<{ ip: string; count: number; country: string }>>((ranking, event) => {
    const existing = ranking.find((item) => item.ip === event.sourceIp);
    if (existing) {
      existing.count += 1;
      return ranking;
    }
    ranking.push({ ip: event.sourceIp || "-", count: 1, country: event.countryName || event.countryCode || "-" });
    return ranking;
  }, []).sort((left, right) => right.count - left.count);

  const wafCountryRanking = wafEvents.reduce<Array<{ code: string; name: string; count: number }>>((ranking, event) => {
    const code = event.countryCode || "UN";
    const existing = ranking.find((item) => item.code === code);
    if (existing) {
      existing.count += 1;
      return ranking;
    }
    ranking.push({ code, name: event.countryName || code, count: 1 });
    return ranking;
  }, []).sort((left, right) => right.count - left.count);

  return (
    <section className="page-grid security-layout">
      <header className="section-header full-span">
        <div>
          <h1>安全管理</h1>
          <p>{status || options.lastApplyMessage || `${rules.length} 条防火墙规则`}</p>
        </div>
        <div className="toolbar">
          <IconButton label="刷新" icon={RefreshCw} onClick={() => void load()} />
          <IconButton label="新建规则" icon={Plus} onClick={() => setRuleForm(defaultFirewallForm)} />
        </div>
      </header>

      <div className="panel security-options">
        <div className="panel-title"><ShieldAlert size={18} /><span>入口防护</span></div>
        <Input
          label="访问路径"
          value={options.panelAccessPath}
          onChange={(panelAccessPath) => setOptions({ ...options, panelAccessPath })}
        />
        <Input
          label="监听地址"
          value={options.panelListenAddr}
          onChange={(panelListenAddr) => setOptions({ ...options, panelListenAddr })}
        />
        <ToggleRow
          label="2FA 登录"
          checked={options.twoFactorRequired}
          onChange={(twoFactorRequired) => setOptions({ ...options, twoFactorRequired })}
        />
        <ToggleRow label="禁 Ping" checked={options.disablePing} onChange={(disablePing) => setOptions({ ...options, disablePing })} />
        <ToggleRow
          label="防扫描"
          checked={options.scanProtectionEnabled}
          onChange={(scanProtectionEnabled) => setOptions({ ...options, scanProtectionEnabled })}
        />
        <NumberInput label="触发次数" value={options.scanBurst} onChange={(scanBurst) => setOptions({ ...options, scanBurst })} />
        <NumberInput
          label="窗口秒数"
          value={options.scanWindowSeconds}
          onChange={(scanWindowSeconds) => setOptions({ ...options, scanWindowSeconds })}
        />
        <SelectRow
          label="后端"
          value={options.backendPreference}
          onChange={(backendPreference) => setOptions({ ...options, backendPreference: Number(backendPreference) as FirewallBackend })}
          options={[
            [FirewallBackend.UNSPECIFIED, "自动检测"],
            [FirewallBackend.UFW, "UFW"],
            [FirewallBackend.FIREWALLD, "Firewalld"],
            [FirewallBackend.IPTABLES, "Iptables"]
          ]}
        />
        <button onClick={() => void saveOptions()} type="button"><Save size={15} />保存开关</button>
      </div>

      <div className="panel rule-form">
        <div className="panel-title"><ShieldCheck size={18} /><span>{ruleForm.id ? "编辑规则" : "新建规则"}</span></div>
        <Input label="名称" value={ruleForm.name} onChange={(name) => setRuleForm({ ...ruleForm, name })} />
        <SelectRow
          label="协议"
          value={ruleForm.protocol}
          onChange={(protocol) => setRuleForm({ ...ruleForm, protocol: Number(protocol) as FirewallProtocol })}
          options={[
            [FirewallProtocol.TCP, "TCP"],
            [FirewallProtocol.UDP, "UDP"],
            [FirewallProtocol.ICMP, "ICMP"]
          ]}
        />
        <SelectRow
          label="动作"
          value={ruleForm.action}
          onChange={(action) => setRuleForm({ ...ruleForm, action: Number(action) as FirewallAction })}
          options={[
            [FirewallAction.ALLOW, "放行"],
            [FirewallAction.DENY, "屏蔽"],
            [FirewallAction.REJECT, "拒绝"]
          ]}
        />
        <SelectRow
          label="方向"
          value={ruleForm.direction}
          onChange={(direction) => setRuleForm({ ...ruleForm, direction: Number(direction) as FirewallDirection })}
          options={[
            [FirewallDirection.INBOUND, "入站"],
            [FirewallDirection.OUTBOUND, "出站"]
          ]}
        />
        {ruleForm.protocol !== FirewallProtocol.ICMP && (
          <div className="inline-grid">
            <Input label="起始端口" type="number" value={ruleForm.portStart} onChange={(portStart) => setRuleForm({ ...ruleForm, portStart })} />
            <Input label="结束端口" type="number" value={ruleForm.portEnd} onChange={(portEnd) => setRuleForm({ ...ruleForm, portEnd })} />
          </div>
        )}
        <Input label="来源 IP/CIDR" value={ruleForm.source} onChange={(source) => setRuleForm({ ...ruleForm, source })} />
        <Input label="目标 IP/CIDR" value={ruleForm.destination} onChange={(destination) => setRuleForm({ ...ruleForm, destination })} />
        <Input label="备注" value={ruleForm.comment} onChange={(comment) => setRuleForm({ ...ruleForm, comment })} />
        <ToggleRow label="启用" checked={ruleForm.enabled} onChange={(enabled) => setRuleForm({ ...ruleForm, enabled })} />
        <button onClick={() => void saveRule()} type="button"><Save size={15} />保存规则</button>
      </div>

      <div className="panel wide-panel firewall-list">
        <div className="panel-title"><Shield size={18} /><span>防火墙规则</span></div>
        <div className="table-list">
          {rules.map((rule) => (
            <div className="table-row firewall-row" key={rule.id}>
              <div>
                <strong>{rule.name}</strong>
                <small>
                  {firewallProtocolLabel(rule.protocol)} · {firewallActionLabel(rule.action)} · {firewallDirectionLabel(rule.direction)}
                  {rule.protocol !== FirewallProtocol.ICMP ? ` · ${rule.portStart}${rule.portEnd && rule.portEnd !== rule.portStart ? `-${rule.portEnd}` : ""}` : ""}
                  {rule.source ? ` · ${rule.source}` : ""}
                </small>
              </div>
              <StatusPill label={rule.enabled ? "启用" : "停用"} tone={rule.enabled ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label={rule.enabled ? "停用" : "启用"} icon={Power} onClick={() => void toggleRule(rule)} />
                <IconButton label="编辑" icon={Copy} onClick={() => editRule(rule)} />
                <IconButton label="删除" icon={Ban} onClick={() => void deleteRule(rule)} />
              </div>
            </div>
          ))}
          {!rules.length && <div className="empty-state">暂无规则</div>}
        </div>
      </div>

      <div className="panel waf-settings">
        <div className="panel-title"><ShieldAlert size={18} /><span>WAF 防护</span></div>
        <ToggleRow label="WAF 总开关" checked={wafSettings.enabled} onChange={(enabled) => setWafSettings({ ...wafSettings, enabled })} />
        <ToggleRow
          label="抗 CC"
          checked={wafSettings.ccProtectionEnabled}
          onChange={(ccProtectionEnabled) => setWafSettings({ ...wafSettings, ccProtectionEnabled })}
        />
        <ToggleRow
          label="验证码挑战"
          checked={wafSettings.captchaChallengeEnabled}
          onChange={(captchaChallengeEnabled) => setWafSettings({ ...wafSettings, captchaChallengeEnabled })}
        />
        <NumberInput label="每分钟请求" value={wafSettings.requestsPerMinute} onChange={(requestsPerMinute) => setWafSettings({ ...wafSettings, requestsPerMinute })} />
        <NumberInput label="突发请求" value={wafSettings.burst} onChange={(burst) => setWafSettings({ ...wafSettings, burst })} />
        <NumberInput
          label="封禁秒数"
          value={wafSettings.blockDurationSeconds}
          onChange={(blockDurationSeconds) => setWafSettings({ ...wafSettings, blockDurationSeconds })}
        />
        <Input label="Nginx 片段" value={wafSettings.nginxConfigPath} onChange={(nginxConfigPath) => setWafSettings({ ...wafSettings, nginxConfigPath })} />
        <Input label="挑战页" value={wafSettings.challengePagePath} onChange={(challengePagePath) => setWafSettings({ ...wafSettings, challengePagePath })} />
        <button onClick={() => void saveWafSettings()} type="button"><Save size={15} />保存 WAF</button>
      </div>

      <div className="panel waf-rule-form">
        <div className="panel-title"><ShieldCheck size={18} /><span>{wafRuleForm.id ? "编辑 WAF 规则" : "新建 WAF 规则"}</span></div>
        <Input label="名称" value={wafRuleForm.name} onChange={(name) => setWafRuleForm({ ...wafRuleForm, name })} />
        <SelectRow
          label="类型"
          value={wafRuleForm.kind}
          onChange={(kind) => setWafRuleForm({ ...wafRuleForm, kind: Number(kind) as WafRuleKind })}
          options={[
            [WafRuleKind.SQL_INJECTION, "SQL 注入"],
            [WafRuleKind.XSS, "XSS"],
            [WafRuleKind.KEYWORD, "关键词"],
            [WafRuleKind.SCANNER, "扫描器"],
            [WafRuleKind.CC, "CC"]
          ]}
        />
        <Input label="匹配规则" value={wafRuleForm.pattern} onChange={(pattern) => setWafRuleForm({ ...wafRuleForm, pattern })} />
        <Input label="站点域名" value={wafRuleForm.scopeDomain} onChange={(scopeDomain) => setWafRuleForm({ ...wafRuleForm, scopeDomain })} />
        <Input label="备注" value={wafRuleForm.comment} onChange={(comment) => setWafRuleForm({ ...wafRuleForm, comment })} />
        <ToggleRow label="启用" checked={wafRuleForm.enabled} onChange={(enabled) => setWafRuleForm({ ...wafRuleForm, enabled })} />
        <button onClick={() => void saveWafRule()} type="button"><Save size={15} />保存规则</button>
      </div>

      <div className="panel wide-panel waf-rule-list">
        <div className="panel-title"><Shield size={18} /><span>WAF 规则库</span></div>
        <div className="table-list">
          {wafRules.map((rule) => (
            <div className="table-row firewall-row" key={rule.id}>
              <div>
                <strong>{rule.name}</strong>
                <small>{wafKindLabel(rule.kind)} · {rule.pattern}{rule.scopeDomain ? ` · ${rule.scopeDomain}` : ""}</small>
              </div>
              <StatusPill label={rule.enabled ? "启用" : "停用"} tone={rule.enabled ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label="编辑" icon={Copy} onClick={() => editWafRule(rule)} />
                <IconButton label="删除" icon={Ban} onClick={() => void deleteWafRule(rule)} />
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel waf-map">
        <div className="panel-title"><Globe size={18} /><span>攻击来源</span></div>
        <div className="world-map" aria-label="WAF attack source map">
          {wafCountryRanking.slice(0, 8).map((country, index) => (
            <span className={`map-point point-${index + 1}`} key={country.code} title={`${country.name}: ${country.count}`}>
              {country.code}
            </span>
          ))}
        </div>
      </div>

      <div className="panel waf-ranking">
        <div className="panel-title"><ShieldAlert size={18} /><span>攻击 IP 排名</span></div>
        <div className="table-list">
          {wafIpRanking.slice(0, 8).map((item) => (
            <div className="rank-row" key={item.ip}>
              <strong>{item.ip}</strong>
              <small>{item.country}</small>
              <StatusPill label={String(item.count)} tone="danger" />
            </div>
          ))}
          {!wafIpRanking.length && <div className="empty-state">暂无拦截记录</div>}
        </div>
      </div>

      <div className="panel ssh-settings">
        <div className="panel-title"><TerminalSquare size={18} /><span>SSH 加固</span></div>
        <ToggleRow label="服务启用" checked={sshSettings.serviceEnabled} onChange={(serviceEnabled) => setSshSettings({ ...sshSettings, serviceEnabled })} />
        <NumberInput label="SSH 端口" value={sshSettings.port} onChange={(port) => setSshSettings({ ...sshSettings, port })} />
        <ToggleRow
          label="禁用密码"
          checked={sshSettings.passwordLoginDisabled}
          onChange={(passwordLoginDisabled) => setSshSettings({ ...sshSettings, passwordLoginDisabled })}
        />
        <ToggleRow label="自动封禁" checked={sshSettings.autoBanEnabled} onChange={(autoBanEnabled) => setSshSettings({ ...sshSettings, autoBanEnabled })} />
        <NumberInput
          label="失败阈值"
          value={sshSettings.failedAttemptLimit}
          onChange={(failedAttemptLimit) => setSshSettings({ ...sshSettings, failedAttemptLimit })}
        />
        <NumberInput
          label="窗口秒数"
          value={sshSettings.failedAttemptWindowSeconds}
          onChange={(failedAttemptWindowSeconds) => setSshSettings({ ...sshSettings, failedAttemptWindowSeconds })}
        />
        <Input label="配置文件" value={sshSettings.configPath} onChange={(configPath) => setSshSettings({ ...sshSettings, configPath })} />
        <button onClick={() => void saveSshSettings()} type="button"><Save size={15} />保存 SSH</button>
      </div>

      <div className="panel ssh-keys">
        <div className="panel-title"><ShieldCheck size={18} /><span>SSH 密钥</span></div>
        <Input label="名称" value={sshKeyForm.name} onChange={(name) => setSshKeyForm({ ...sshKeyForm, name })} />
        <SelectRow
          label="算法"
          value={sshKeyForm.algorithm}
          onChange={(algorithm) => setSshKeyForm({ ...sshKeyForm, algorithm: Number(algorithm) as SshKeyAlgorithm })}
          options={[
            [SshKeyAlgorithm.ED25519, "Ed25519"],
            [SshKeyAlgorithm.RSA, "RSA 4096"]
          ]}
        />
        <button onClick={() => void generateSshKey()} type="button"><Plus size={15} />生成</button>
        <div className="table-list compact-list">
          {sshKeys.map((key) => (
            <div className="key-row" key={key.id}>
              <strong>{key.name}</strong>
              <small>{sshAlgorithmLabel(key.algorithm)} · {key.privateKeyPath}</small>
            </div>
          ))}
          {!sshKeys.length && <div className="empty-state">暂无密钥</div>}
        </div>
      </div>

      <div className="panel full-span ssh-audit">
        <div className="panel-title"><FileText size={18} /><span>SSH 登录审计</span></div>
        <div className="table-list">
          {sshEvents.slice(0, 12).map((event) => (
            <div className="table-row firewall-row" key={event.id}>
              <div>
                <strong>{event.username} · {event.sourceIp || "-"}</strong>
                <small>{event.message || new Date(Number(event.occurredAtSeconds) * 1000).toLocaleString()}</small>
              </div>
              <StatusPill label={event.successful ? "成功" : "失败"} tone={event.successful ? "good" : "danger"} />
              <StatusPill label={event.autoBanned ? "已封禁" : "未封禁"} tone={event.autoBanned ? "danger" : "muted"} />
            </div>
          ))}
          {!sshEvents.length && <div className="empty-state">暂无审计记录</div>}
        </div>
      </div>

      <div className="panel backup-panel full-span">
        <div className="panel-title"><FileDown size={18} /><span>规则备份</span></div>
        <div className="toolbar backup-actions">
          <button onClick={() => void exportRules()} type="button"><FileDown size={15} />导出</button>
          <button onClick={() => void importRules()} type="button"><FileUp size={15} />导入覆盖</button>
        </div>
        <textarea
          onChange={(event) => setBackupJson(event.target.value)}
          spellCheck={false}
          value={backupJson}
        />
      </div>
    </section>
  );
}
