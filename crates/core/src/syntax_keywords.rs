// Keyword data ported from the original IdeaCodeHighlighter.java.
pub(super) const LANGUAGES: &[(&str, &str)] = &[
    (
        "java",
        "abstract assert boolean break byte case catch char class const continue default do double else enum extends final finally float for goto if implements import instanceof int interface long native new package private protected public return short static strictfp super switch synchronized this throw throws transient try void volatile while true false null var yield record sealed permits non-sealed",
    ),
    (
        "kotlin",
        "as break class continue do else false for fun if in interface is null object package return super this throw true try typealias val var when while by catch finally get import init out override private protected public internal sealed data lateinit open abstract companion inline operator infix crossinline suspend tailrec vararg reified",
    ),
    (
        "scala",
        "abstract case catch class def do else extends false final finally for if implicit import lazy match new null object override package private protected return sealed super this throw trait try true type val var while with yield given using enum export then",
    ),
    (
        "javascript",
        "break case catch class const continue debugger default delete do else export extends finally for function if import in instanceof new return super switch this throw try typeof var void while with yield let static true false null undefined async await of as",
    ),
    (
        "typescript",
        "break case catch class const continue debugger default delete do else export extends finally for function if import in instanceof new return super switch this throw try typeof var void while with yield let static true false null undefined async await of as interface type enum implements private protected public readonly abstract is keyof infer namespace declare module symbol bigint never unknown any satisfies",
    ),
    (
        "python",
        "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield print match case self cls",
    ),
    (
        "sql",
        "select where insert update delete create table drop alter into values set join left right inner outer full cross group by order having limit offset distinct primary key foreign references index unique between like exists union all and or not in is as from on using with case when then else end if begin commit rollback grant revoke database schema view trigger procedure function null true false asc desc count sum avg min max over partition window returning",
    ),
    (
        "go",
        "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var true false nil iota make len cap new append panic recover",
    ),
    (
        "rust",
        "as break const continue crate else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while async await dyn union box macro yield",
    ),
    (
        "cpp",
        "alignas alignof and auto bool break case catch char class const constexpr consteval constinit continue co_await co_return co_yield decltype default delete do double else enum explicit export extern false float for friend goto if inline int long mutable namespace new noexcept nullptr operator or private protected public register reinterpret_cast return short signed sizeof static static_cast struct switch template this thread_local throw true try typedef typename union unsigned using virtual void volatile while std size_t",
    ),
    (
        "c",
        "auto break case char const continue default do double else enum extern float for goto if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while NULL size_t",
    ),
    (
        "csharp",
        "abstract as base bool break byte case catch char checked class const continue decimal default delegate do double else enum event explicit extern false finally fixed float for foreach goto if implicit in int interface internal is lock long namespace new null object operator out override params private protected public readonly ref return sbyte sealed short sizeof stackalloc static string struct switch this throw true try typeof uint ulong unchecked unsafe ushort using virtual void volatile while var async await yield get set record init required",
    ),
    (
        "php",
        "abstract and array as break callable case catch class clone const continue declare default die do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile eval exit extends final finally fn for foreach function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public require require_once return static switch throw trait try unset use var while xor yield true false null",
    ),
    (
        "shell",
        "if then else elif fi for do done while until case esac in function return break continue exit echo printf read local declare export unset source alias shift test true false cd pwd set trap",
    ),
    (
        "powershell",
        "begin break catch class continue data define do dynamicparam else elseif end enum exit filter finally for foreach from function if in param process return switch throw trap try until using var while workflow true false null",
    ),
    (
        "ruby",
        "BEGIN END alias and begin break case class def defined do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require include extend",
    ),
    (
        "swift",
        "associatedtype class deinit enum extension fileprivate func import init inout internal let open operator private protocol public static struct subscript typealias var break case continue default defer do else fallthrough for guard if in repeat return switch where while as Any catch false is nil rethrows super self Self throw throws true try async await actor some any",
    ),
    (
        "dart",
        "abstract as assert async await base break case catch class const continue covariant default deferred do dynamic else enum export extends extension external factory false final finally for function get hide if implements import in interface is late library mixin new null of on operator part required rethrow return sealed set show static super switch sync this throw true try type typedef var void when while with yield",
    ),
    (
        "groovy",
        "as assert break case catch class const continue def default do else enum extends false finally for goto if implements import in instanceof interface new null package return super switch this throw throws trait true try var while public protected private static final abstract synchronized",
    ),
    (
        "lua",
        "and break do else elseif end false for function goto if in local nil not or repeat return then true until while",
    ),
    (
        "css",
        "important inherit initial unset revert none auto block inline flex grid absolute relative fixed sticky transparent currentColor var calc rgb rgba hsl hsla",
    ),
];
