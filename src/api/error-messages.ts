// =====================================================================
// 错误码到中文文案的本地化映射
// 与 src-tauri/src/errors.rs 的 GitViewError variants 一一对应。
// 新增 variant 时务必同步本文件，否则 UI 将回退到通用文案。
// =====================================================================

/**
 * 错误码字面量联合类型。
 *
 * 与 Rust 端 `GitViewError` 的 variant 名一一对应（`#[serde(tag = "code")]`）。
 */
export type GitViewErrorCode =
  | 'Database'
  | 'Credential'
  | 'TokenInvalid'
  | 'ApiUrlInvalid'
  | 'Network'
  | 'TlsCert'
  | 'ResponseDecode'
  | 'Forbidden'
  | 'NotFound'
  | 'GitCommand'
  | 'GitNotFound'
  | 'PathConflict'
  | 'RepoNameTaken'
  | 'PathMissing'
  | 'UserCancelled'
  | 'BusyAccount'
  | 'DirtyWorkdir'
  | 'TagNameInvalid'
  | 'TagAlreadyExists'
  | 'BranchNotFound'
  | 'AnnotatedTagUnsupported'
  | 'RateLimited'
  | 'Internal';

/**
 * 错误码到中文用户文案的映射表。
 *
 * 文案原则：用户视角描述、可操作（含下一步建议时尽量给出）、不暴露内部细节。
 */
const ERROR_MESSAGES: Record<GitViewErrorCode, string> = {
  Database: '数据库错误',
  Credential: '凭据存储错误，请检查系统密钥库是否可用',
  TokenInvalid: 'Token 无效或已过期，请重新配置账号',
  ApiUrlInvalid: 'API 地址格式错误，请检查实例 URL',
  Network: '网络异常，请检查网络连接或代理设置',
  TlsCert: 'TLS 证书校验失败，可在账号设置中确认是否信任自签名证书',
  ResponseDecode: '服务器返回了非预期的数据格式，请检查 API 地址是否正确或实例版本是否兼容',
  Forbidden: '权限不足，无法访问该资源',
  NotFound: '资源不存在',
  GitCommand: 'Git 命令执行失败',
  GitNotFound: '未找到 Git 可执行文件，请在设置中指定路径',
  PathConflict: '路径冲突，请选择其他目录',
  RepoNameTaken: '该名称的仓库已存在，请改用其他名称',
  PathMissing: '路径不存在',
  UserCancelled: '操作已取消',
  BusyAccount: '该账号正在同步中，请稍后再试',
  DirtyWorkdir: '工作区存在未提交变更，请先提交或暂存后再执行',
  TagNameInvalid: 'Tag 名称不符合 Git ref 规则',
  TagAlreadyExists: '目标项目中已存在同名 Tag，不会覆盖',
  BranchNotFound: '目标远程分支不存在或已被删除',
  AnnotatedTagUnsupported: '当前平台或实例不支持创建附注 Tag',
  RateLimited: '平台 API 请求过于频繁，请稍后重试失败项目',
  Internal: '内部错误，请重试或查看日志获取详情',
};

/**
 * 把错误码（以及可选的后端详细信息）渲染为中文用户文案。
 *
 * 后端附带的 `detail` 会以括号形式追加在主文案后，便于排查；
 * 但 detail 本身在 Rust 端已经过脱敏，不应包含敏感凭据。
 */
export function localizeError(code: GitViewErrorCode, detail?: string): string {
  const base = ERROR_MESSAGES[code] ?? `未知错误（${code}）`;
  return detail ? `${base}（${detail}）` : base;
}
