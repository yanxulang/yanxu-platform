//! 有界、可验证且不含平台句柄的无障碍语义树。

use crate::data::Data;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;

pub const ACCESSIBILITY_MAJOR: i64 = 1;
pub const ACCESSIBILITY_MINOR: i64 = 0;
pub const MAX_SEMANTIC_NODES: usize = 16_384;
pub const MAX_SEMANTIC_DEPTH: usize = 64;
pub const MAX_SEMANTIC_CHILDREN: usize = 4_096;
pub const MAX_SEMANTIC_ACTIONS: usize = 16;
pub const MAX_SEMANTIC_NODE_TEXT_BYTES: usize = 65_536;
pub const MAX_SEMANTIC_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_SEMANTIC_COORDINATE: f64 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilitySource {
    AssistiveTechnology,
}

impl AccessibilitySource {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AssistiveTechnology => "辅助技术",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticNode {
    id: i64,
    role: String,
    name: String,
    description: String,
    value: Data,
    states: BTreeMap<String, Data>,
    actions: Vec<String>,
    bounds: [f64; 4],
    children: Vec<Self>,
}

impl SemanticNode {
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    #[must_use]
    pub fn to_data(&self) -> Data {
        Data::map([
            ("编号", Data::Integer(self.id)),
            ("角色", Data::String(self.role.clone())),
            ("名称", Data::String(self.name.clone())),
            ("描述", Data::String(self.description.clone())),
            ("值", self.value.clone()),
            ("状态", Data::Map(self.states.clone())),
            (
                "操作",
                Data::Array(self.actions.iter().cloned().map(Data::String).collect()),
            ),
            (
                "边界",
                Data::Array(self.bounds.into_iter().map(Data::Number).collect()),
            ),
            (
                "子",
                Data::Array(self.children.iter().map(Self::to_data).collect()),
            ),
        ])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticNodeSummary {
    pub role: String,
    pub parent: Option<i64>,
    pub states: BTreeMap<String, Data>,
    pub actions: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticTree {
    root: SemanticNode,
    nodes: BTreeMap<i64, SemanticNodeSummary>,
    text_bytes: usize,
    focused: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccessibilityState {
    revision: i64,
    tree: Option<Arc<SemanticTree>>,
}

impl AccessibilityState {
    pub fn replace(&mut self, tree: Option<SemanticTree>) -> Result<bool, AccessibilityError> {
        if self.tree.as_deref() == tree.as_ref() {
            return Ok(false);
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(AccessibilityError::Revision)?;
        self.revision = revision;
        self.tree = tree.map(Arc::new);
        Ok(true)
    }

    #[must_use]
    pub const fn revision(&self) -> i64 {
        self.revision
    }

    #[must_use]
    pub fn tree(&self) -> Option<&SemanticTree> {
        self.tree.as_deref()
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.tree().map_or(0, SemanticTree::node_count)
    }

    #[must_use]
    pub fn text_bytes(&self) -> usize {
        self.tree().map_or(0, SemanticTree::text_bytes)
    }

    #[must_use]
    pub fn focused(&self) -> Option<i64> {
        self.tree().and_then(SemanticTree::focused)
    }

    pub fn focus_target(&self, id: i64) -> Result<&SemanticNodeSummary, AccessibilityError> {
        self.tree()
            .ok_or(AccessibilityError::Node(id))?
            .focus_target(id)
    }
}

impl SemanticTree {
    pub fn validate(value: &Data) -> Result<Self, AccessibilityError> {
        let mut context = ValidationContext::default();
        let root = parse_node(value, None, 0, &mut context)?;
        Ok(Self {
            root,
            nodes: context.nodes,
            text_bytes: context.text_bytes,
            focused: context.focused,
        })
    }

    #[must_use]
    pub fn root(&self) -> &SemanticNode {
        &self.root
    }

    #[must_use]
    pub fn node(&self, id: i64) -> Option<&SemanticNodeSummary> {
        self.nodes.get(&id)
    }

    pub fn focus_target(&self, id: i64) -> Result<&SemanticNodeSummary, AccessibilityError> {
        let node = self.node(id).ok_or(AccessibilityError::Node(id))?;
        if node.actions.contains("聚焦")
            && matches!(node.states.get("可聚焦"), Some(Data::Bool(true)))
            && !matches!(node.states.get("启用"), Some(Data::Bool(false)))
            && !matches!(node.states.get("可见"), Some(Data::Bool(false)))
        {
            Ok(node)
        } else {
            Err(AccessibilityError::Focus(id))
        }
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub const fn text_bytes(&self) -> usize {
        self.text_bytes
    }

    #[must_use]
    pub const fn focused(&self) -> Option<i64> {
        self.focused
    }

    #[must_use]
    pub fn to_data(&self) -> Data {
        self.root.to_data()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessibilityError {
    Tree,
    Limit(&'static str),
    Duplicate(i64),
    Node(i64),
    Role(String),
    State(String),
    Action(String),
    Focus(i64),
    Revision,
}

impl AccessibilityError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Tree => "PLATFORM_ACCESSIBILITY_TREE",
            Self::Limit(_) => "PLATFORM_ACCESSIBILITY_LIMIT",
            Self::Duplicate(_) => "PLATFORM_ACCESSIBILITY_DUPLICATE",
            Self::Node(_) => "PLATFORM_ACCESSIBILITY_NODE",
            Self::Role(_) => "PLATFORM_ACCESSIBILITY_ROLE",
            Self::State(_) => "PLATFORM_ACCESSIBILITY_STATE",
            Self::Action(_) => "PLATFORM_ACCESSIBILITY_ACTION",
            Self::Focus(_) => "PLATFORM_ACCESSIBILITY_FOCUS",
            Self::Revision => "PLATFORM_ACCESSIBILITY_REVISION",
        }
    }
}

impl Display for AccessibilityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tree => formatter.write_str("无障碍语义树结构无效"),
            Self::Limit(name) => write!(formatter, "无障碍语义树超过{name}上限"),
            Self::Duplicate(id) => write!(formatter, "无障碍节点编号 {id} 重复"),
            Self::Node(id) => write!(formatter, "无障碍节点编号 {id} 不存在"),
            Self::Role(role) => write!(formatter, "无障碍角色无效：{role}"),
            Self::State(state) => write!(formatter, "无障碍状态无效：{state}"),
            Self::Action(action) => write!(formatter, "无障碍操作无效：{action}"),
            Self::Focus(id) => write!(formatter, "无障碍焦点状态无效：{id}"),
            Self::Revision => formatter.write_str("无障碍树修订已耗尽"),
        }
    }
}

impl Error for AccessibilityError {}

#[derive(Default)]
struct ValidationContext {
    nodes: BTreeMap<i64, SemanticNodeSummary>,
    text_bytes: usize,
    focused: Option<i64>,
}

fn parse_node(
    value: &Data,
    parent: Option<i64>,
    depth: usize,
    context: &mut ValidationContext,
) -> Result<SemanticNode, AccessibilityError> {
    if depth > MAX_SEMANTIC_DEPTH {
        return Err(AccessibilityError::Limit("深度"));
    }
    if context.nodes.len() >= MAX_SEMANTIC_NODES {
        return Err(AccessibilityError::Limit("节点数"));
    }
    let Data::Map(map) = value else {
        return Err(AccessibilityError::Tree);
    };
    let id = positive_id(map.get("编号"))?;
    if context.nodes.contains_key(&id) {
        return Err(AccessibilityError::Duplicate(id));
    }
    let role = required_text(map.get("角色"))?;
    if !valid_role(role) {
        return Err(AccessibilityError::Role(role.to_owned()));
    }
    let name = optional_text(map.get("名称"), context)?;
    let description = optional_text(map.get("描述"), context)?;
    let value = semantic_value(map.get("值"), context)?;
    let states = semantic_states(map.get("状态"), role, id, context)?;
    let actions = semantic_actions(map.get("操作"), role, &states)?;
    let bounds = semantic_bounds(map.get("边界"))?;
    context.nodes.insert(
        id,
        SemanticNodeSummary {
            role: role.to_owned(),
            parent,
            states: states.clone(),
            actions: actions.iter().cloned().collect(),
        },
    );
    let children = semantic_children(map.get("子"), id, depth, context)?;
    Ok(SemanticNode {
        id,
        role: role.to_owned(),
        name,
        description,
        value,
        states,
        actions,
        bounds,
        children,
    })
}

fn positive_id(value: Option<&Data>) -> Result<i64, AccessibilityError> {
    let Some(Data::Integer(id)) = value else {
        return Err(AccessibilityError::Tree);
    };
    if *id > 0 {
        Ok(*id)
    } else {
        Err(AccessibilityError::Tree)
    }
}

fn required_text(value: Option<&Data>) -> Result<&str, AccessibilityError> {
    let Some(Data::String(value)) = value else {
        return Err(AccessibilityError::Tree);
    };
    if value.len() > MAX_SEMANTIC_NODE_TEXT_BYTES {
        return Err(AccessibilityError::Limit("单字段文字"));
    }
    Ok(value)
}

fn optional_text(
    value: Option<&Data>,
    context: &mut ValidationContext,
) -> Result<String, AccessibilityError> {
    let value = match value {
        None | Some(Data::Nil) => "",
        Some(Data::String(value)) => value,
        _ => return Err(AccessibilityError::Tree),
    };
    record_text(value, context)?;
    Ok(value.to_owned())
}

fn record_text(value: &str, context: &mut ValidationContext) -> Result<(), AccessibilityError> {
    if value.len() > MAX_SEMANTIC_NODE_TEXT_BYTES {
        return Err(AccessibilityError::Limit("单字段文字"));
    }
    context.text_bytes = context
        .text_bytes
        .checked_add(value.len())
        .filter(|total| *total <= MAX_SEMANTIC_TEXT_BYTES)
        .ok_or(AccessibilityError::Limit("文字总量"))?;
    Ok(())
}

fn semantic_value(
    value: Option<&Data>,
    context: &mut ValidationContext,
) -> Result<Data, AccessibilityError> {
    match value.unwrap_or(&Data::Nil) {
        Data::Nil => Ok(Data::Nil),
        Data::Bool(value) => Ok(Data::Bool(*value)),
        Data::Integer(value) => Ok(Data::Integer(*value)),
        Data::Number(value) if value.is_finite() => Ok(Data::Number(*value)),
        Data::String(value) => {
            record_text(value, context)?;
            Ok(Data::String(value.clone()))
        }
        _ => Err(AccessibilityError::Tree),
    }
}

fn semantic_states(
    value: Option<&Data>,
    role: &str,
    id: i64,
    context: &mut ValidationContext,
) -> Result<BTreeMap<String, Data>, AccessibilityError> {
    let states = match value {
        None | Some(Data::Nil) => return Ok(BTreeMap::new()),
        Some(Data::Map(states)) => states,
        _ => return Err(AccessibilityError::State("状态必须是典".to_owned())),
    };
    for (name, value) in states {
        validate_state(name, value, role)?;
    }
    let focused = state_bool(states, "焦点")?;
    if focused {
        validate_focused_node(states, id, context)?;
    }
    if state_bool(states, "混合")? && state_bool(states, "已检查")? {
        return Err(AccessibilityError::State(
            "混合与已检查不能同时为真".to_owned(),
        ));
    }
    Ok(states.clone())
}

fn validate_state(name: &str, value: &Data, role: &str) -> Result<(), AccessibilityError> {
    if name.len() > 32 {
        return Err(AccessibilityError::State("状态名称过长".to_owned()));
    }
    match name {
        "启用" | "可见" | "可聚焦" | "焦点" | "忙碌" => require_bool(name, value),
        "只读" | "必填" | "无效" if form_role(role) => require_bool(name, value),
        "选中" | "当前" if selectable_role(role) => require_bool(name, value),
        "已检查" if checkable_role(role) => require_bool(name, value),
        "混合" if role == "复选框" => require_bool(name, value),
        "展开" if expandable_role(role) => require_bool(name, value),
        "多选" if multi_select_role(role) => require_bool(name, value),
        "模态" if matches!(role, "对话框" | "弹出层") => require_bool(name, value),
        "按下" if matches!(role, "按钮" | "切换") => require_bool(name, value),
        "方向" if oriented_role(role) => require_orientation(value),
        "级别" if matches!(role, "标题" | "树项" | "行") => require_level(value),
        _ => Err(AccessibilityError::State(format!("{role}.{name}"))),
    }
}

fn require_bool(name: &str, value: &Data) -> Result<(), AccessibilityError> {
    if matches!(value, Data::Bool(_)) {
        Ok(())
    } else {
        Err(AccessibilityError::State(name.to_owned()))
    }
}

fn require_orientation(value: &Data) -> Result<(), AccessibilityError> {
    if matches!(value, Data::String(value) if matches!(value.as_str(), "横向" | "纵向")) {
        Ok(())
    } else {
        Err(AccessibilityError::State("方向".to_owned()))
    }
}

fn require_level(value: &Data) -> Result<(), AccessibilityError> {
    if matches!(value, Data::Integer(value) if (1..=64).contains(value)) {
        Ok(())
    } else {
        Err(AccessibilityError::State("级别".to_owned()))
    }
}

fn state_bool(states: &BTreeMap<String, Data>, name: &str) -> Result<bool, AccessibilityError> {
    match states.get(name) {
        None => Ok(false),
        Some(Data::Bool(value)) => Ok(*value),
        Some(_) => Err(AccessibilityError::State(name.to_owned())),
    }
}

fn validate_focused_node(
    states: &BTreeMap<String, Data>,
    id: i64,
    context: &mut ValidationContext,
) -> Result<(), AccessibilityError> {
    if matches!(states.get("启用"), Some(Data::Bool(false)))
        || matches!(states.get("可见"), Some(Data::Bool(false)))
        || !matches!(states.get("可聚焦"), Some(Data::Bool(true)))
        || context.focused.is_some()
    {
        return Err(AccessibilityError::Focus(id));
    }
    context.focused = Some(id);
    Ok(())
}

fn semantic_actions(
    value: Option<&Data>,
    role: &str,
    states: &BTreeMap<String, Data>,
) -> Result<Vec<String>, AccessibilityError> {
    let actions = match value {
        None | Some(Data::Nil) => return Ok(Vec::new()),
        Some(Data::Array(actions)) => actions,
        _ => return Err(AccessibilityError::Action("操作必须是列".to_owned())),
    };
    if actions.len() > MAX_SEMANTIC_ACTIONS {
        return Err(AccessibilityError::Limit("单节点操作数"));
    }
    let mut result = Vec::with_capacity(actions.len());
    let mut unique = BTreeSet::new();
    for action in actions {
        let Data::String(action) = action else {
            return Err(AccessibilityError::Action("操作名称必须是文".to_owned()));
        };
        if action.len() > 32 {
            return Err(AccessibilityError::Action("操作名称过长".to_owned()));
        }
        if !valid_action(action, role, states) || !unique.insert(action.clone()) {
            return Err(AccessibilityError::Action(format!("{role}.{action}")));
        }
        result.push(action.clone());
    }
    Ok(result)
}

fn valid_action(action: &str, role: &str, states: &BTreeMap<String, Data>) -> bool {
    match action {
        "聚焦" => matches!(states.get("可聚焦"), Some(Data::Bool(true))),
        "点击" => clickable_role(role),
        "设置值" => value_role(role),
        "选择" => selectable_role(role) || text_role(role),
        "取消选择" => selectable_role(role),
        "复制" | "剪切" | "粘贴" => text_role(role),
        "展开" | "折叠" => expandable_role(role),
        "增加" | "减少" => adjustable_role(role),
        "滚动" => scrollable_role(role),
        "滚动到" => true,
        "显示菜单" => matches!(role, "按钮" | "输入框" | "多行输入框" | "密码框"),
        _ => false,
    }
}

fn semantic_bounds(value: Option<&Data>) -> Result<[f64; 4], AccessibilityError> {
    let Some(Data::Array(values)) = value else {
        return Err(AccessibilityError::Tree);
    };
    if values.len() != 4 {
        return Err(AccessibilityError::Tree);
    }
    let mut bounds = [0.0; 4];
    for (index, value) in values.iter().enumerate() {
        let number = value.as_number().filter(|number| number.is_finite());
        bounds[index] = number.ok_or(AccessibilityError::Tree)?;
    }
    if bounds[0].abs() > MAX_SEMANTIC_COORDINATE
        || bounds[1].abs() > MAX_SEMANTIC_COORDINATE
        || !(0.0..=MAX_SEMANTIC_COORDINATE).contains(&bounds[2])
        || !(0.0..=MAX_SEMANTIC_COORDINATE).contains(&bounds[3])
    {
        return Err(AccessibilityError::Tree);
    }
    Ok(bounds)
}

fn semantic_children(
    value: Option<&Data>,
    parent: i64,
    depth: usize,
    context: &mut ValidationContext,
) -> Result<Vec<SemanticNode>, AccessibilityError> {
    let children = match value {
        None | Some(Data::Nil) => return Ok(Vec::new()),
        Some(Data::Array(children)) => children,
        _ => return Err(AccessibilityError::Tree),
    };
    if children.len() > MAX_SEMANTIC_CHILDREN {
        return Err(AccessibilityError::Limit("单节点子数"));
    }
    children
        .iter()
        .map(|child| parse_node(child, Some(parent), depth + 1, context))
        .collect()
}

fn valid_role(role: &str) -> bool {
    matches!(
        role,
        "窗口"
            | "组"
            | "面板"
            | "控件"
            | "文字"
            | "标题"
            | "按钮"
            | "链接"
            | "复选框"
            | "单选框"
            | "切换"
            | "输入框"
            | "多行输入框"
            | "密码框"
            | "组合框"
            | "列表"
            | "列表项"
            | "树"
            | "树项"
            | "标签页"
            | "标签"
            | "菜单栏"
            | "菜单"
            | "菜单项"
            | "弹出层"
            | "对话框"
            | "滑块"
            | "进度条"
            | "滚动条"
            | "滚动容器"
            | "分割面板"
            | "工具栏"
            | "画布"
            | "图片"
            | "表格"
            | "行"
            | "单元格"
            | "分隔符"
    )
}

fn form_role(role: &str) -> bool {
    text_role(role) || matches!(role, "组合框" | "复选框" | "单选框" | "切换" | "滑块")
}

fn text_role(role: &str) -> bool {
    matches!(role, "输入框" | "多行输入框" | "密码框")
}

fn selectable_role(role: &str) -> bool {
    matches!(
        role,
        "列表项" | "树项" | "标签" | "菜单项" | "行" | "单元格" | "单选框"
    )
}

fn checkable_role(role: &str) -> bool {
    matches!(role, "复选框" | "单选框" | "切换" | "菜单项")
}

fn expandable_role(role: &str) -> bool {
    matches!(role, "组合框" | "树项" | "菜单" | "菜单项" | "弹出层")
}

fn multi_select_role(role: &str) -> bool {
    matches!(role, "列表" | "树" | "标签页" | "表格")
}

fn oriented_role(role: &str) -> bool {
    matches!(role, "滑块" | "滚动条" | "分割面板" | "工具栏" | "分隔符")
}

fn clickable_role(role: &str) -> bool {
    matches!(
        role,
        "按钮" | "链接" | "复选框" | "单选框" | "切换" | "标签" | "菜单" | "菜单项"
    )
}

fn value_role(role: &str) -> bool {
    text_role(role) || matches!(role, "组合框" | "滑块" | "滚动条" | "分割面板")
}

fn adjustable_role(role: &str) -> bool {
    matches!(role, "滑块" | "滚动条" | "分割面板")
}

fn scrollable_role(role: &str) -> bool {
    matches!(role, "滚动容器" | "列表" | "树" | "表格")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(
        id: i64,
        role: &str,
        states: BTreeMap<String, Data>,
        actions: Vec<&str>,
        children: Vec<Data>,
    ) -> Data {
        Data::map([
            ("编号", Data::Integer(id)),
            ("角色", Data::String(role.to_owned())),
            ("名称", Data::String(format!("节点{id}"))),
            ("描述", Data::String(String::new())),
            ("值", Data::Nil),
            ("状态", Data::Map(states)),
            (
                "操作",
                Data::Array(
                    actions
                        .into_iter()
                        .map(|action| Data::String(action.to_owned()))
                        .collect(),
                ),
            ),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 100.into(), 30.into()]),
            ),
            ("子", Data::Array(children)),
        ])
    }

    #[test]
    fn validates_and_indexes_a_ui_semantic_tree() {
        let button = node(
            2,
            "按钮",
            BTreeMap::from([
                ("启用".to_owned(), Data::Bool(true)),
                ("可聚焦".to_owned(), Data::Bool(true)),
            ]),
            vec!["点击", "聚焦"],
            Vec::new(),
        );
        let input = node(
            3,
            "输入框",
            BTreeMap::from([
                ("启用".to_owned(), Data::Bool(true)),
                ("可见".to_owned(), Data::Bool(true)),
                ("可聚焦".to_owned(), Data::Bool(true)),
                ("焦点".to_owned(), Data::Bool(true)),
            ]),
            vec!["设置值", "选择", "复制", "粘贴"],
            Vec::new(),
        );
        let value = node(1, "面板", BTreeMap::new(), Vec::new(), vec![button, input]);
        let tree = SemanticTree::validate(&value).unwrap();
        assert_eq!(tree.node_count(), 3);
        assert_eq!(tree.focused(), Some(3));
        assert_eq!(tree.node(2).unwrap().parent, Some(1));
        assert!(tree.node(2).unwrap().actions.contains("点击"));
        assert_eq!(tree.root().id(), 1);
        assert_eq!(SemanticTree::validate(&tree.to_data()).unwrap(), tree);
    }

    #[test]
    fn rejects_duplicate_or_non_positive_node_ids() {
        let duplicate = node(1, "按钮", BTreeMap::new(), vec!["点击"], Vec::new());
        let tree = node(1, "面板", BTreeMap::new(), Vec::new(), vec![duplicate]);
        assert_eq!(
            SemanticTree::validate(&tree).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_DUPLICATE"
        );
        assert_eq!(
            SemanticTree::validate(&node(0, "面板", BTreeMap::new(), Vec::new(), Vec::new(),))
                .unwrap_err()
                .code(),
            "PLATFORM_ACCESSIBILITY_TREE"
        );
    }

    #[test]
    fn validates_roles_states_and_role_action_pairs() {
        let unknown = node(1, "未知", BTreeMap::new(), Vec::new(), Vec::new());
        assert_eq!(
            SemanticTree::validate(&unknown).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_ROLE"
        );
        let invalid_state = node(
            1,
            "文字",
            BTreeMap::from([("展开".to_owned(), Data::Bool(true))]),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            SemanticTree::validate(&invalid_state).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_STATE"
        );
        let invalid_action = node(1, "图片", BTreeMap::new(), vec!["设置值"], Vec::new());
        assert_eq!(
            SemanticTree::validate(&invalid_action).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_ACTION"
        );
    }

    #[test]
    fn enforces_a_single_enabled_visible_focus() {
        let focused = |id| {
            node(
                id,
                "按钮",
                BTreeMap::from([
                    ("启用".to_owned(), Data::Bool(true)),
                    ("可见".to_owned(), Data::Bool(true)),
                    ("可聚焦".to_owned(), Data::Bool(true)),
                    ("焦点".to_owned(), Data::Bool(true)),
                ]),
                vec!["点击"],
                Vec::new(),
            )
        };
        let tree = node(
            1,
            "面板",
            BTreeMap::new(),
            Vec::new(),
            vec![focused(2), focused(3)],
        );
        assert_eq!(
            SemanticTree::validate(&tree).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_FOCUS"
        );
        let hidden_focus = node(
            1,
            "按钮",
            BTreeMap::from([
                ("可见".to_owned(), Data::Bool(false)),
                ("焦点".to_owned(), Data::Bool(true)),
            ]),
            vec!["点击"],
            Vec::new(),
        );
        assert_eq!(
            SemanticTree::validate(&hidden_focus).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_FOCUS"
        );
    }

    #[test]
    fn bounds_depth_children_and_text() {
        let oversized_text = Data::map([
            ("编号", Data::Integer(1)),
            ("角色", Data::String("文字".to_owned())),
            (
                "名称",
                Data::String("x".repeat(MAX_SEMANTIC_NODE_TEXT_BYTES + 1)),
            ),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 1.into(), 1.into()]),
            ),
        ]);
        assert_eq!(
            SemanticTree::validate(&oversized_text).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_LIMIT"
        );

        let children = (2..=MAX_SEMANTIC_CHILDREN as i64 + 2)
            .map(|id| node(id, "文字", BTreeMap::new(), Vec::new(), Vec::new()))
            .collect();
        let too_wide = node(1, "面板", BTreeMap::new(), Vec::new(), children);
        assert_eq!(
            SemanticTree::validate(&too_wide).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_LIMIT"
        );

        let mut too_deep = node(100, "文字", BTreeMap::new(), Vec::new(), Vec::new());
        for id in (1..=MAX_SEMANTIC_DEPTH as i64 + 1).rev() {
            too_deep = node(id, "面板", BTreeMap::new(), Vec::new(), vec![too_deep]);
        }
        assert_eq!(
            SemanticTree::validate(&too_deep).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_LIMIT"
        );
    }

    #[test]
    fn window_state_deduplicates_updates_and_clears_idempotently() {
        let value = node(1, "面板", BTreeMap::new(), Vec::new(), Vec::new());
        let mut state = AccessibilityState::default();
        assert!(
            state
                .replace(Some(SemanticTree::validate(&value).unwrap()))
                .unwrap()
        );
        assert_eq!(state.revision(), 1);
        assert_eq!(state.node_count(), 1);
        assert!(
            !state
                .replace(Some(SemanticTree::validate(&value).unwrap()))
                .unwrap()
        );
        assert_eq!(state.revision(), 1);
        assert!(state.replace(None).unwrap());
        assert_eq!(state.revision(), 2);
        assert_eq!(state.node_count(), 0);
        assert!(!state.replace(None).unwrap());
        assert_eq!(state.revision(), 2);
    }

    #[test]
    fn focus_requests_require_an_enabled_visible_advertised_target() {
        let value = node(
            1,
            "按钮",
            BTreeMap::from([
                ("启用".to_owned(), Data::Bool(true)),
                ("可见".to_owned(), Data::Bool(true)),
                ("可聚焦".to_owned(), Data::Bool(true)),
            ]),
            vec!["点击", "聚焦"],
            Vec::new(),
        );
        let tree = SemanticTree::validate(&value).unwrap();
        assert_eq!(tree.focus_target(1).unwrap().role, "按钮");
        assert_eq!(
            tree.focus_target(2).unwrap_err().code(),
            "PLATFORM_ACCESSIBILITY_NODE"
        );

        let disabled = node(
            1,
            "按钮",
            BTreeMap::from([
                ("启用".to_owned(), Data::Bool(false)),
                ("可聚焦".to_owned(), Data::Bool(true)),
            ]),
            vec!["聚焦"],
            Vec::new(),
        );
        assert_eq!(
            SemanticTree::validate(&disabled)
                .unwrap()
                .focus_target(1)
                .unwrap_err()
                .code(),
            "PLATFORM_ACCESSIBILITY_FOCUS"
        );
    }
}
