import { IconButton, Input, NumberInput, SelectRow, StatusPill, ToggleRow } from "../components/form-controls";
import { FirewallAction, FirewallBackend, FirewallDirection, FirewallProtocol, FirewallRule, SshKeyAlgorithm, SshKeyItem, SshLoginEvent, WafAttackEvent, WafRule, WafRuleKind } from "../gen/rustpanel/v1/security_pb";
import { formatDateTime, safeError } from "../lib/format";
import { defaultFirewallForm, defaultSecurityOptions, defaultSshKeyForm, defaultSshSettings, defaultWafRuleForm, defaultWafSettings, type FirewallForm, type SecurityOptionsForm, type SshKeyForm, type SshSettingsForm, type WafRuleForm, type WafSettingsForm } from "../lib/forms";
import { firewallActionLabel, firewallDirectionLabel, firewallProtocolLabel, sshAlgorithmLabel, wafKindLabel } from "../lib/labels";
import { type Clients } from "../lib/rpc";
import { Ban, Copy, FileDown, FileText, FileUp, Globe, Plus, Power, RefreshCw, Save, Shield, ShieldAlert, ShieldCheck, TerminalSquare } from "lucide-react";
import { useState } from "react";
import { useMountEffect } from "../lib/hooks";
import { useLocale } from "../lib/i18n/locale-provider";

export function SecurityPanel({ clients }: { clients: Clients }) {
  const { t, locale } = useLocale();
  const [rules, setRules] = useState<FirewallRule[]>([]);
  const [ruleForm, setRuleForm] = useState<FirewallForm>(defaultFirewallForm());
  const [options, setOptions] = useState<SecurityOptionsForm>(defaultSecurityOptions);
  const [wafSettings, setWafSettings] = useState<WafSettingsForm>(defaultWafSettings);
  const [wafRules, setWafRules] = useState<WafRule[]>([]);
  const [wafRuleForm, setWafRuleForm] = useState<WafRuleForm>(defaultWafRuleForm());
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
      setRuleForm(defaultFirewallForm());
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
      setStatus(response.options?.lastApplyMessage ?? t("security.optionsSaved"));
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const exportRules = async () => {
    try {
      const response = await clients.security.exportFirewallRules({});
      setBackupJson(response.backupJson);
      setStatus(t("security.backupExported"));
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
      setStatus(t("security.backupImported"));
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveWafSettings = async () => {
    try {
      const response = await clients.security.updateWafSettings({ settings: wafSettings });
      if (response.settings) {
        setWafSettings({ ...defaultWafSettings, ...response.settings });
        setStatus(response.settings.lastApplyMessage || t("security.wafSaved"));
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
      setWafRuleForm(defaultWafRuleForm());
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
        setStatus(response.settings.lastApplyMessage || t("security.sshSaved"));
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
          <h1>{t("security.title")}</h1>
          <p>{status || options.lastApplyMessage || t("security.ruleCount", { count: rules.length })}</p>
        </div>
        <div className="toolbar">
          <IconButton label={t("security.refresh")} icon={RefreshCw} onClick={() => void load()} />
          <IconButton label={t("security.newRule")} icon={Plus} onClick={() => setRuleForm(defaultFirewallForm())} />
        </div>
      </header>

      <div className="panel security-options">
        <div className="panel-title"><ShieldAlert size={18} /><span>{t("security.entryProtection")}</span></div>
        <Input
          label={t("security.accessPath")}
          value={options.panelAccessPath}
          onChange={(panelAccessPath) => setOptions({ ...options, panelAccessPath })}
        />
        <Input
          label={t("security.listenAddr")}
          value={options.panelListenAddr}
          onChange={(panelListenAddr) => setOptions({ ...options, panelListenAddr })}
        />
        <ToggleRow
          label={t("security.twoFactorLogin")}
          checked={options.twoFactorRequired}
          onChange={(twoFactorRequired) => setOptions({ ...options, twoFactorRequired })}
        />
        <ToggleRow
          label={t("security.disablePing")}
          checked={options.disablePing}
          onChange={(disablePing) => setOptions({ ...options, disablePing })}
        />
        <ToggleRow
          label={t("security.antiScan")}
          checked={options.scanProtectionEnabled}
          onChange={(scanProtectionEnabled) => setOptions({ ...options, scanProtectionEnabled })}
        />
        <NumberInput
          label={t("security.triggerCount")}
          value={options.scanBurst}
          onChange={(scanBurst) => setOptions({ ...options, scanBurst })}
        />
        <NumberInput
          label={t("security.windowSeconds")}
          value={options.scanWindowSeconds}
          onChange={(scanWindowSeconds) => setOptions({ ...options, scanWindowSeconds })}
        />
        <SelectRow
          label={t("security.backend")}
          value={options.backendPreference}
          onChange={(backendPreference) => setOptions({ ...options, backendPreference: Number(backendPreference) as FirewallBackend })}
          options={[
            [FirewallBackend.UNSPECIFIED, t("security.autoDetect")],
            [FirewallBackend.UFW, "UFW"],
            [FirewallBackend.FIREWALLD, "Firewalld"],
            [FirewallBackend.IPTABLES, "Iptables"]
          ]}
        />
        <button onClick={() => void saveOptions()} type="button">
          <Save size={15} />
          {t("security.saveToggles")}
        </button>
      </div>

      <div className="panel rule-form">
        <div className="panel-title">
          <ShieldCheck size={18} />
          <span>{ruleForm.id ? t("security.editRule") : t("security.newRule")}</span>
        </div>
        <Input label={t("security.name")} value={ruleForm.name} onChange={(name) => setRuleForm({ ...ruleForm, name })} />
        <SelectRow
          label={t("security.protocol")}
          value={ruleForm.protocol}
          onChange={(protocol) => setRuleForm({ ...ruleForm, protocol: Number(protocol) as FirewallProtocol })}
          options={[
            [FirewallProtocol.TCP, "TCP"],
            [FirewallProtocol.UDP, "UDP"],
            [FirewallProtocol.ICMP, "ICMP"]
          ]}
        />
        <SelectRow
          label={t("security.action")}
          value={ruleForm.action}
          onChange={(action) => setRuleForm({ ...ruleForm, action: Number(action) as FirewallAction })}
          options={[
            [FirewallAction.ALLOW, firewallActionLabel(FirewallAction.ALLOW, t)],
            [FirewallAction.DENY, firewallActionLabel(FirewallAction.DENY, t)],
            [FirewallAction.REJECT, firewallActionLabel(FirewallAction.REJECT, t)]
          ]}
        />
        <SelectRow
          label={t("security.direction")}
          value={ruleForm.direction}
          onChange={(direction) => setRuleForm({ ...ruleForm, direction: Number(direction) as FirewallDirection })}
          options={[
            [FirewallDirection.INBOUND, firewallDirectionLabel(FirewallDirection.INBOUND, t)],
            [FirewallDirection.OUTBOUND, firewallDirectionLabel(FirewallDirection.OUTBOUND, t)]
          ]}
        />
        {ruleForm.protocol !== FirewallProtocol.ICMP && (
          <div className="inline-grid">
            <Input
              label={t("security.startPort")}
              type="number"
              value={ruleForm.portStart}
              onChange={(portStart) => setRuleForm({ ...ruleForm, portStart })}
            />
            <Input
              label={t("security.endPort")}
              type="number"
              value={ruleForm.portEnd}
              onChange={(portEnd) => setRuleForm({ ...ruleForm, portEnd })}
            />
          </div>
        )}
        <Input label={t("security.sourceIpCidr")} value={ruleForm.source} onChange={(source) => setRuleForm({ ...ruleForm, source })} />
        <Input
          label={t("security.destIpCidr")}
          value={ruleForm.destination}
          onChange={(destination) => setRuleForm({ ...ruleForm, destination })}
        />
        <Input label={t("security.comment")} value={ruleForm.comment} onChange={(comment) => setRuleForm({ ...ruleForm, comment })} />
        <ToggleRow label={t("security.enabled")} checked={ruleForm.enabled} onChange={(enabled) => setRuleForm({ ...ruleForm, enabled })} />
        <button onClick={() => void saveRule()} type="button">
          <Save size={15} />
          {t("security.saveRule")}
        </button>
      </div>

      <div className="panel wide-panel firewall-list">
        <div className="panel-title"><Shield size={18} /><span>{t("security.firewallRules")}</span></div>
        <div className="table-list">
          {rules.map((rule) => (
            <div className="table-row firewall-row" key={rule.id}>
              <div>
                <strong>{rule.name}</strong>
                <small>
                  {firewallProtocolLabel(rule.protocol)} · {firewallActionLabel(rule.action, t)} ·{" "}
                  {firewallDirectionLabel(rule.direction, t)}
                  {rule.protocol !== FirewallProtocol.ICMP ? ` · ${rule.portStart}${rule.portEnd && rule.portEnd !== rule.portStart ? `-${rule.portEnd}` : ""}` : ""}
                  {rule.source ? ` · ${rule.source}` : ""}
                </small>
              </div>
              <StatusPill
                label={rule.enabled ? t("security.enabled") : t("security.disabled")}
                tone={rule.enabled ? "good" : "muted"}
              />
              <div className="row-actions">
                <IconButton
                  label={rule.enabled ? t("security.disabled") : t("security.enabled")}
                  icon={Power}
                  onClick={() => void toggleRule(rule)}
                />
                <IconButton label={t("security.edit")} icon={Copy} onClick={() => editRule(rule)} />
                <IconButton label={t("security.delete")} icon={Ban} onClick={() => void deleteRule(rule)} />
              </div>
            </div>
          ))}
          {!rules.length && <div className="empty-state">{t("security.noRules")}</div>}
        </div>
      </div>

      <div className="panel waf-settings">
        <div className="panel-title"><ShieldAlert size={18} /><span>{t("security.wafProtection")}</span></div>
        <ToggleRow
          label={t("security.wafMasterSwitch")}
          checked={wafSettings.enabled}
          onChange={(enabled) => setWafSettings({ ...wafSettings, enabled })}
        />
        <ToggleRow
          label={t("security.antiCc")}
          checked={wafSettings.ccProtectionEnabled}
          onChange={(ccProtectionEnabled) => setWafSettings({ ...wafSettings, ccProtectionEnabled })}
        />
        <ToggleRow
          label={t("security.captchaChallenge")}
          checked={wafSettings.captchaChallengeEnabled}
          onChange={(captchaChallengeEnabled) => setWafSettings({ ...wafSettings, captchaChallengeEnabled })}
        />
        <NumberInput
          label={t("security.requestsPerMinute")}
          value={wafSettings.requestsPerMinute}
          onChange={(requestsPerMinute) => setWafSettings({ ...wafSettings, requestsPerMinute })}
        />
        <NumberInput
          label={t("security.burstRequests")}
          value={wafSettings.burst}
          onChange={(burst) => setWafSettings({ ...wafSettings, burst })}
        />
        <NumberInput
          label={t("security.blockSeconds")}
          value={wafSettings.blockDurationSeconds}
          onChange={(blockDurationSeconds) => setWafSettings({ ...wafSettings, blockDurationSeconds })}
        />
        <Input
          label={t("security.nginxFragment")}
          value={wafSettings.nginxConfigPath}
          onChange={(nginxConfigPath) => setWafSettings({ ...wafSettings, nginxConfigPath })}
        />
        <Input
          label={t("security.challengePage")}
          value={wafSettings.challengePagePath}
          onChange={(challengePagePath) => setWafSettings({ ...wafSettings, challengePagePath })}
        />
        <button onClick={() => void saveWafSettings()} type="button">
          <Save size={15} />
          {t("security.saveWaf")}
        </button>
      </div>

      <div className="panel waf-rule-form">
        <div className="panel-title">
          <ShieldCheck size={18} />
          <span>{wafRuleForm.id ? t("security.editWafRule") : t("security.newWafRule")}</span>
        </div>
        <Input label={t("security.name")} value={wafRuleForm.name} onChange={(name) => setWafRuleForm({ ...wafRuleForm, name })} />
        <SelectRow
          label={t("security.kind")}
          value={wafRuleForm.kind}
          onChange={(kind) => setWafRuleForm({ ...wafRuleForm, kind: Number(kind) as WafRuleKind })}
          options={[
            [WafRuleKind.SQL_INJECTION, wafKindLabel(WafRuleKind.SQL_INJECTION, t)],
            [WafRuleKind.XSS, wafKindLabel(WafRuleKind.XSS, t)],
            [WafRuleKind.KEYWORD, wafKindLabel(WafRuleKind.KEYWORD, t)],
            [WafRuleKind.SCANNER, wafKindLabel(WafRuleKind.SCANNER, t)],
            [WafRuleKind.CC, wafKindLabel(WafRuleKind.CC, t)]
          ]}
        />
        <Input
          label={t("security.matchPattern")}
          value={wafRuleForm.pattern}
          onChange={(pattern) => setWafRuleForm({ ...wafRuleForm, pattern })}
        />
        <Input
          label={t("security.scopeDomain")}
          value={wafRuleForm.scopeDomain}
          onChange={(scopeDomain) => setWafRuleForm({ ...wafRuleForm, scopeDomain })}
        />
        <Input label={t("security.comment")} value={wafRuleForm.comment} onChange={(comment) => setWafRuleForm({ ...wafRuleForm, comment })} />
        <ToggleRow
          label={t("security.enabled")}
          checked={wafRuleForm.enabled}
          onChange={(enabled) => setWafRuleForm({ ...wafRuleForm, enabled })}
        />
        <button onClick={() => void saveWafRule()} type="button">
          <Save size={15} />
          {t("security.saveRule")}
        </button>
      </div>

      <div className="panel wide-panel waf-rule-list">
        <div className="panel-title"><Shield size={18} /><span>{t("security.wafRuleLibrary")}</span></div>
        <div className="table-list">
          {wafRules.map((rule) => (
            <div className="table-row firewall-row" key={rule.id}>
              <div>
                <strong>{rule.name}</strong>
                <small>
                  {wafKindLabel(rule.kind, t)} · {rule.pattern}
                  {rule.scopeDomain ? ` · ${rule.scopeDomain}` : ""}
                </small>
              </div>
              <StatusPill
                label={rule.enabled ? t("security.enabled") : t("security.disabled")}
                tone={rule.enabled ? "good" : "muted"}
              />
              <div className="row-actions">
                <IconButton label={t("security.edit")} icon={Copy} onClick={() => editWafRule(rule)} />
                <IconButton label={t("security.delete")} icon={Ban} onClick={() => void deleteWafRule(rule)} />
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel waf-map">
        <div className="panel-title"><Globe size={18} /><span>{t("security.attackSources")}</span></div>
        <div className="world-map" aria-label="WAF attack source map">
          {wafCountryRanking.slice(0, 8).map((country, index) => (
            <span className={`map-point point-${index + 1}`} key={country.code} title={`${country.name}: ${country.count}`}>
              {country.code}
            </span>
          ))}
        </div>
      </div>

      <div className="panel waf-ranking">
        <div className="panel-title"><ShieldAlert size={18} /><span>{t("security.attackIpRanking")}</span></div>
        <div className="table-list">
          {wafIpRanking.slice(0, 8).map((item) => (
            <div className="rank-row" key={item.ip}>
              <strong>{item.ip}</strong>
              <small>{item.country}</small>
              <StatusPill label={String(item.count)} tone="danger" />
            </div>
          ))}
          {!wafIpRanking.length && <div className="empty-state">{t("security.noBlockRecords")}</div>}
        </div>
      </div>

      <div className="panel ssh-settings">
        <div className="panel-title"><TerminalSquare size={18} /><span>{t("security.sshHardening")}</span></div>
        <ToggleRow
          label={t("security.serviceEnabled")}
          checked={sshSettings.serviceEnabled}
          onChange={(serviceEnabled) => setSshSettings({ ...sshSettings, serviceEnabled })}
        />
        <NumberInput label={t("security.sshPort")} value={sshSettings.port} onChange={(port) => setSshSettings({ ...sshSettings, port })} />
        <ToggleRow
          label={t("security.disablePasswordLogin")}
          checked={sshSettings.passwordLoginDisabled}
          onChange={(passwordLoginDisabled) => setSshSettings({ ...sshSettings, passwordLoginDisabled })}
        />
        <ToggleRow
          label={t("security.autoBan")}
          checked={sshSettings.autoBanEnabled}
          onChange={(autoBanEnabled) => setSshSettings({ ...sshSettings, autoBanEnabled })}
        />
        <NumberInput
          label={t("security.failThreshold")}
          value={sshSettings.failedAttemptLimit}
          onChange={(failedAttemptLimit) => setSshSettings({ ...sshSettings, failedAttemptLimit })}
        />
        <NumberInput
          label={t("security.windowSeconds")}
          value={sshSettings.failedAttemptWindowSeconds}
          onChange={(failedAttemptWindowSeconds) => setSshSettings({ ...sshSettings, failedAttemptWindowSeconds })}
        />
        <Input
          label={t("security.configFile")}
          value={sshSettings.configPath}
          onChange={(configPath) => setSshSettings({ ...sshSettings, configPath })}
        />
        <button onClick={() => void saveSshSettings()} type="button">
          <Save size={15} />
          {t("security.saveSsh")}
        </button>
      </div>

      <div className="panel ssh-keys">
        <div className="panel-title"><ShieldCheck size={18} /><span>{t("security.sshKeys")}</span></div>
        <Input label={t("security.name")} value={sshKeyForm.name} onChange={(name) => setSshKeyForm({ ...sshKeyForm, name })} />
        <SelectRow
          label={t("security.algorithm")}
          value={sshKeyForm.algorithm}
          onChange={(algorithm) => setSshKeyForm({ ...sshKeyForm, algorithm: Number(algorithm) as SshKeyAlgorithm })}
          options={[
            [SshKeyAlgorithm.ED25519, "Ed25519"],
            [SshKeyAlgorithm.RSA, "RSA 4096"]
          ]}
        />
        <button onClick={() => void generateSshKey()} type="button">
          <Plus size={15} />
          {t("security.generate")}
        </button>
        <div className="table-list compact-list">
          {sshKeys.map((key) => (
            <div className="key-row" key={key.id}>
              <strong>{key.name}</strong>
              <small>{sshAlgorithmLabel(key.algorithm)} · {key.privateKeyPath}</small>
            </div>
          ))}
          {!sshKeys.length && <div className="empty-state">{t("security.noKeys")}</div>}
        </div>
      </div>

      <div className="panel full-span ssh-audit">
        <div className="panel-title"><FileText size={18} /><span>{t("security.sshAudit")}</span></div>
        <div className="table-list">
          {sshEvents.slice(0, 12).map((event) => (
            <div className="table-row firewall-row" key={event.id}>
              <div>
                <strong>{event.username} · {event.sourceIp || "-"}</strong>
                <small>
                  {event.message || formatDateTime(new Date(Number(event.occurredAtSeconds) * 1000), locale)}
                </small>
              </div>
              <StatusPill
                label={event.successful ? t("security.success") : t("security.failure")}
                tone={event.successful ? "good" : "danger"}
              />
              <StatusPill
                label={event.autoBanned ? t("security.banned") : t("security.notBanned")}
                tone={event.autoBanned ? "danger" : "muted"}
              />
            </div>
          ))}
          {!sshEvents.length && <div className="empty-state">{t("security.noAuditRecords")}</div>}
        </div>
      </div>

      <div className="panel backup-panel full-span">
        <div className="panel-title"><FileDown size={18} /><span>{t("security.ruleBackup")}</span></div>
        <div className="toolbar backup-actions">
          <button onClick={() => void exportRules()} type="button">
            <FileDown size={15} />
            {t("security.export")}
          </button>
          <button onClick={() => void importRules()} type="button">
            <FileUp size={15} />
            {t("security.importOverwrite")}
          </button>
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
