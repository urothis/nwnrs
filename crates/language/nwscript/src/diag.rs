use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

/// Stable compiler and VM error codes used by the upstream `NWScript` compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(i32)]
pub enum CompilerErrorCode {
    /// An unexpected character was encountered during lexing.
    UnexpectedCharacter = -560,
    /// The compiler reported a fatal internal error.
    FatalCompilerError = -561,
    /// Parsing expected a compound statement at program start.
    ProgramCompoundStatementAtStart = -562,
    /// A closing brace appeared without a matching opening brace.
    UnexpectedEndCompoundStatement = -563,
    /// Unexpected tokens appeared after a compound statement.
    AfterCompoundStatementAtEnd = -564,
    /// Parsing a variable list failed.
    ParsingVariableList = -565,
    /// The compiler entered an unexpected internal state.
    UnknownStateInCompiler = -566,
    /// A declaration used an invalid type.
    InvalidDeclarationType = -567,
    /// An expression was missing `(`.
    NoLeftBracketOnExpression = -568,
    /// An expression was missing `)`.
    NoRightBracketOnExpression = -569,
    /// A statement started with an invalid token.
    BadStartOfStatement = -570,
    /// A call argument list was missing `(`.
    NoLeftBracketOnArgList = -571,
    /// A call argument list was missing `)`.
    NoRightBracketOnArgList = -572,
    /// An expression statement was missing a semicolon.
    NoSemicolonAfterExpression = -573,
    /// Parsing an assignment statement failed.
    ParsingAssignmentStatement = -574,
    /// The assignment target was invalid.
    BadLValue = -575,
    /// A constant literal had an invalid type.
    BadConstantType = -576,
    /// The identifier table is full.
    IdentifierListFull = -577,
    /// An integer constant referenced a non-integer identifier.
    NonIntegerIdForIntegerConstant = -578,
    /// A float constant referenced a non-float identifier.
    NonFloatIdForFloatConstant = -579,
    /// A string constant referenced a non-string identifier.
    NonStringIdForStringConstant = -580,
    /// A variable name was reused within the same scope.
    VariableAlreadyUsedWithinScope = -581,
    /// A variable was defined without a type.
    VariableDefinedWithoutType = -582,
    /// The compile stack ended in an invalid variable state.
    IncorrectVariableStateLeftOnStack = -583,
    /// An integer expression was required.
    NonIntegerExpressionWhereIntegerRequired = -584,
    /// A non-void expression was required.
    VoidExpressionWhereNonVoidRequired = -585,
    /// Assignment parameters were invalid.
    InvalidParametersForAssignment = -586,
    /// A declaration did not match its parameter list.
    DeclarationDoesNotMatchParameters = -587,
    /// A logical operation used invalid operands.
    LogicalOperationHasInvalidOperands = -588,
    /// An equality test used invalid operands.
    EqualityTestHasInvalidOperands = -589,
    /// A comparison test used invalid operands.
    ComparisonTestHasInvalidOperands = -590,
    /// A shift operation used invalid operands.
    ShiftOperationHasInvalidOperands = -591,
    /// An arithmetic operation used invalid operands.
    ArithmeticOperationHasInvalidOperands = -592,
    /// The semantic checker saw an unknown operation.
    UnknownOperationInSemanticCheck = -593,
    /// The compiled script exceeded the maximum supported size.
    ScriptTooLarge = -594,
    /// A return statement was missing its return value.
    ReturnStatementHasNoParameters = -595,
    /// `do` was not followed by `while`.
    NoWhileAfterDoKeyword = -596,
    /// A function definition was missing its name.
    FunctionDefinitionMissingName = -597,
    /// A function definition was missing its parameter list.
    FunctionDefinitionMissingParameterList = -598,
    /// A parameter list was malformed.
    MalformedParameterList = -599,
    /// A type specifier was invalid.
    BadTypeSpecifier = -600,
    /// A struct declaration was missing a semicolon.
    NoSemicolonAfterStructure = -601,
    /// An ellipsis-like construct appeared in an identifier.
    EllipsisInIdentifier = -602,
    /// A requested source file could not be found.
    FileNotFound = -603,
    /// A recursive include was detected.
    IncludeRecursive = -604,
    /// Too many include levels were used.
    IncludeTooManyLevels = -605,
    /// Parsing a return statement failed.
    ParsingReturnStatement = -606,
    /// Parsing the identifier list failed.
    ParsingIdentifierList = -607,
    /// Parsing a function declaration failed.
    ParsingFunctionDeclaration = -608,
    /// A function implementation was defined more than once.
    DuplicateFunctionImplementation = -609,
    /// A token exceeded the maximum allowed length.
    TokenTooLong = -610,
    /// A referenced struct type was undefined.
    UndefinedStructure = -611,
    /// The left side of a field access was not a structure.
    LeftOfStructurePartNotStructure = -612,
    /// The right side of a field access was not a valid field.
    RightOfStructurePartNotFieldInStructure = -613,
    /// A referenced struct field was undefined.
    UndefinedFieldInStructure = -614,
    /// A struct was redefined.
    StructureRedefined = -615,
    /// A field name was reused within the same structure.
    VariableUsedTwiceInSameStructure = -616,
    /// A function implementation disagreed with its declaration.
    FunctionImplementationAndDefinitionDiffer = -617,
    /// Two types were incompatible.
    MismatchedTypes = -618,
    /// The top of stack was not an integer when required.
    IntegerNotAtTopOfStack = -619,
    /// A function return type and returned expression type disagreed.
    ReturnTypeAndFunctionTypeMismatched = -620,
    /// Not all control paths return a value.
    NotAllControlPathsReturnAValue = -621,
    /// An identifier was undefined.
    UndefinedIdentifier = -622,
    /// The script did not define `main`.
    NoFunctionMainInScript = -623,
    /// `main` must return `void`.
    FunctionMainMustHaveVoidReturnValue = -624,
    /// `main` must not take parameters.
    FunctionMainMustHaveNoParameters = -625,
    /// A non-void function call was used as a statement.
    NonVoidFunctionCannotBeAStatement = -626,
    /// A variable name was invalid.
    BadVariableName = -627,
    /// A required parameter followed an optional one.
    NonOptionalParameterCannotFollowOptionalParameter = -628,
    /// A type does not support optional parameters.
    TypeDoesNotHaveAnOptionalParameter = -629,
    /// A function declaration used a non-constant default value.
    NonConstantInFunctionDeclaration = -630,
    /// Parsing a constant vector failed.
    ParsingConstantVector = -631,
    /// An operand needed to be an integer lvalue.
    OperandMustBeAnIntegerLValue = -1594,
    /// A conditional expression was missing its second expression.
    ConditionalRequiresSecondExpression = -1595,
    /// Both arms of a conditional expression must agree on type.
    ConditionalMustHaveMatchingReturnTypes = -1596,
    /// Multiple `default` labels appeared in one switch.
    MultipleDefaultStatementsWithinSwitch = -1597,
    /// The same `case` value appeared more than once in one switch.
    MultipleCaseConstantStatementsWithinSwitch = -1598,
    /// A `case` value was not a constant integer.
    CaseParameterNotAConstantInteger = -1599,
    /// A `switch` expression must evaluate to an integer.
    SwitchMustEvaluateToAnInteger = -1600,
    /// A `default` label was missing its colon.
    NoColonAfterDefaultLabel = -1601,
    /// A `case` label was missing its colon.
    NoColonAfterCaseLabel = -1602,
    /// A statement was missing its semicolon.
    NoSemicolonAfterStatement = -1603,
    /// `break` was used outside a loop or switch case.
    BreakOutsideOfLoopOrCaseStatement = -4834,
    /// A function exceeded the maximum number of parameters.
    TooManyParametersOnFunction = -4835,
    /// An output file could not be written.
    UnableToOpenFileForWriting = -4836,
    /// A string literal was unterminated.
    UnterminatedStringConstant = -4855,
    /// The script did not define the conditional entry function.
    NoFunctionIntscInScript = -5182,
    /// The conditional entry function must return `void`.
    FunctionIntscMustHaveVoidReturnValue = -5183,
    /// The conditional entry function must not take parameters.
    FunctionIntscMustHaveNoParameters = -5184,
    /// A `case` jump would cross declarations illegally.
    JumpingOverDeclarationStatementsCaseDisallowed = -6804,
    /// A `default` jump would cross declarations illegally.
    JumpingOverDeclarationStatementsDefaultDisallowed = -6805,
    /// `else` appeared without a matching `if`.
    ElseWithoutCorrespondingIf = -6823,
    /// An `if` condition cannot be followed by an empty statement.
    IfConditionCannotBeFollowedByANullStatement = -10407,
    /// `const` was used with an invalid type.
    InvalidTypeForConstKeyword = -3741,
    /// `const` cannot be used on non-global variables.
    ConstKeywordCannotBeUsedOnNonGlobalVariables = -3742,
    /// A constant declaration used an invalid assigned value.
    InvalidValueAssignedToConstant = -3752,
    /// A `switch` condition cannot be followed by an empty statement.
    SwitchConditionCannotBeFollowedByANullStatement = -9081,
    /// A `while` condition cannot be followed by an empty statement.
    WhileConditionCannotBeFollowedByANullStatement = -9082,
    /// A `for` statement cannot be followed by an empty statement.
    ForStatementCannotBeFollowedByANullStatement = -9083,
    /// The same file cannot be included twice.
    CannotIncludeThisFileTwice = -9155,
    /// `else` cannot be followed by an empty statement.
    ElseCannotBeFollowedByANullStatement = -40104,
    /// The VM exceeded its instruction limit.
    VmTooManyInstructions = -632,
    /// The VM exceeded its recursion limit.
    VmTooManyLevelsOfRecursion = -633,
    /// The VM could not open a script file.
    VmFileNotOpened = -634,
    /// The VM was asked to run an uncompiled file.
    VmFileNotCompiledSuccessfully = -635,
    /// The VM encountered an invalid aux code.
    VmInvalidAuxCode = -636,
    /// The VM encountered a null node.
    VmNullVirtualMachineNode = -637,
    /// The VM stack overflowed.
    VmStackOverflow = -638,
    /// The VM stack underflowed.
    VmStackUnderflow = -639,
    /// The VM encountered an invalid opcode.
    VmInvalidOpCode = -640,
    /// The VM encountered invalid extra data for an opcode.
    VmInvalidExtraDataOnOpCode = -641,
    /// The VM encountered an invalid command id.
    VmInvalidCommand = -642,
    /// The VM hit a fake shortcut logical operation.
    VmFakeShortcutLogicalOperation = -643,
    /// The VM attempted division by zero.
    VmDivideByZero = -644,
    /// The VM received a fake abort request.
    VmFakeAbortScript = -645,
    /// The VM instruction pointer left the code segment.
    VmIpOutOfCodeSegment = -646,
    /// The VM command implementer was not configured.
    VmCommandImplementerNotSet = -647,
    /// The VM encountered an unknown stack value type.
    VmUnknownTypeOnRunTimeStack = -648,
    /// The underlying error has already been emitted.
    AlreadyPrinted = -1,
}

impl CompilerErrorCode {
    /// Returns the stable integer code used in diagnostics and fixtures.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// An error returned when a numeric compiler error code is not recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCompilerErrorCode {
    code: i32,
}

impl UnknownCompilerErrorCode {
    /// Creates a new unknown-code error.
    #[must_use]
    pub fn new(code: i32) -> Self {
        Self {
            code,
        }
    }

    /// Returns the unrecognized numeric code.
    #[must_use]
    pub fn code(&self) -> i32 {
        self.code
    }
}

impl fmt::Display for UnknownCompilerErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown NWScript compiler error code: {}", self.code)
    }
}

impl Error for UnknownCompilerErrorCode {}

impl TryFrom<i32> for CompilerErrorCode {
    type Error = UnknownCompilerErrorCode;

    fn try_from(code: i32) -> Result<Self, Self::Error> {
        let value = match code {
            -560 => Self::UnexpectedCharacter,
            -561 => Self::FatalCompilerError,
            -562 => Self::ProgramCompoundStatementAtStart,
            -563 => Self::UnexpectedEndCompoundStatement,
            -564 => Self::AfterCompoundStatementAtEnd,
            -565 => Self::ParsingVariableList,
            -566 => Self::UnknownStateInCompiler,
            -567 => Self::InvalidDeclarationType,
            -568 => Self::NoLeftBracketOnExpression,
            -569 => Self::NoRightBracketOnExpression,
            -570 => Self::BadStartOfStatement,
            -571 => Self::NoLeftBracketOnArgList,
            -572 => Self::NoRightBracketOnArgList,
            -573 => Self::NoSemicolonAfterExpression,
            -574 => Self::ParsingAssignmentStatement,
            -575 => Self::BadLValue,
            -576 => Self::BadConstantType,
            -577 => Self::IdentifierListFull,
            -578 => Self::NonIntegerIdForIntegerConstant,
            -579 => Self::NonFloatIdForFloatConstant,
            -580 => Self::NonStringIdForStringConstant,
            -581 => Self::VariableAlreadyUsedWithinScope,
            -582 => Self::VariableDefinedWithoutType,
            -583 => Self::IncorrectVariableStateLeftOnStack,
            -584 => Self::NonIntegerExpressionWhereIntegerRequired,
            -585 => Self::VoidExpressionWhereNonVoidRequired,
            -586 => Self::InvalidParametersForAssignment,
            -587 => Self::DeclarationDoesNotMatchParameters,
            -588 => Self::LogicalOperationHasInvalidOperands,
            -589 => Self::EqualityTestHasInvalidOperands,
            -590 => Self::ComparisonTestHasInvalidOperands,
            -591 => Self::ShiftOperationHasInvalidOperands,
            -592 => Self::ArithmeticOperationHasInvalidOperands,
            -593 => Self::UnknownOperationInSemanticCheck,
            -594 => Self::ScriptTooLarge,
            -595 => Self::ReturnStatementHasNoParameters,
            -596 => Self::NoWhileAfterDoKeyword,
            -597 => Self::FunctionDefinitionMissingName,
            -598 => Self::FunctionDefinitionMissingParameterList,
            -599 => Self::MalformedParameterList,
            -600 => Self::BadTypeSpecifier,
            -601 => Self::NoSemicolonAfterStructure,
            -602 => Self::EllipsisInIdentifier,
            -603 => Self::FileNotFound,
            -604 => Self::IncludeRecursive,
            -605 => Self::IncludeTooManyLevels,
            -606 => Self::ParsingReturnStatement,
            -607 => Self::ParsingIdentifierList,
            -608 => Self::ParsingFunctionDeclaration,
            -609 => Self::DuplicateFunctionImplementation,
            -610 => Self::TokenTooLong,
            -611 => Self::UndefinedStructure,
            -612 => Self::LeftOfStructurePartNotStructure,
            -613 => Self::RightOfStructurePartNotFieldInStructure,
            -614 => Self::UndefinedFieldInStructure,
            -615 => Self::StructureRedefined,
            -616 => Self::VariableUsedTwiceInSameStructure,
            -617 => Self::FunctionImplementationAndDefinitionDiffer,
            -618 => Self::MismatchedTypes,
            -619 => Self::IntegerNotAtTopOfStack,
            -620 => Self::ReturnTypeAndFunctionTypeMismatched,
            -621 => Self::NotAllControlPathsReturnAValue,
            -622 => Self::UndefinedIdentifier,
            -623 => Self::NoFunctionMainInScript,
            -624 => Self::FunctionMainMustHaveVoidReturnValue,
            -625 => Self::FunctionMainMustHaveNoParameters,
            -626 => Self::NonVoidFunctionCannotBeAStatement,
            -627 => Self::BadVariableName,
            -628 => Self::NonOptionalParameterCannotFollowOptionalParameter,
            -629 => Self::TypeDoesNotHaveAnOptionalParameter,
            -630 => Self::NonConstantInFunctionDeclaration,
            -631 => Self::ParsingConstantVector,
            -1594 => Self::OperandMustBeAnIntegerLValue,
            -1595 => Self::ConditionalRequiresSecondExpression,
            -1596 => Self::ConditionalMustHaveMatchingReturnTypes,
            -1597 => Self::MultipleDefaultStatementsWithinSwitch,
            -1598 => Self::MultipleCaseConstantStatementsWithinSwitch,
            -1599 => Self::CaseParameterNotAConstantInteger,
            -1600 => Self::SwitchMustEvaluateToAnInteger,
            -1601 => Self::NoColonAfterDefaultLabel,
            -1602 => Self::NoColonAfterCaseLabel,
            -1603 => Self::NoSemicolonAfterStatement,
            -4834 => Self::BreakOutsideOfLoopOrCaseStatement,
            -4835 => Self::TooManyParametersOnFunction,
            -4836 => Self::UnableToOpenFileForWriting,
            -4855 => Self::UnterminatedStringConstant,
            -5182 => Self::NoFunctionIntscInScript,
            -5183 => Self::FunctionIntscMustHaveVoidReturnValue,
            -5184 => Self::FunctionIntscMustHaveNoParameters,
            -6804 => Self::JumpingOverDeclarationStatementsCaseDisallowed,
            -6805 => Self::JumpingOverDeclarationStatementsDefaultDisallowed,
            -6823 => Self::ElseWithoutCorrespondingIf,
            -10407 => Self::IfConditionCannotBeFollowedByANullStatement,
            -3741 => Self::InvalidTypeForConstKeyword,
            -3742 => Self::ConstKeywordCannotBeUsedOnNonGlobalVariables,
            -3752 => Self::InvalidValueAssignedToConstant,
            -9081 => Self::SwitchConditionCannotBeFollowedByANullStatement,
            -9082 => Self::WhileConditionCannotBeFollowedByANullStatement,
            -9083 => Self::ForStatementCannotBeFollowedByANullStatement,
            -9155 => Self::CannotIncludeThisFileTwice,
            -40104 => Self::ElseCannotBeFollowedByANullStatement,
            -632 => Self::VmTooManyInstructions,
            -633 => Self::VmTooManyLevelsOfRecursion,
            -634 => Self::VmFileNotOpened,
            -635 => Self::VmFileNotCompiledSuccessfully,
            -636 => Self::VmInvalidAuxCode,
            -637 => Self::VmNullVirtualMachineNode,
            -638 => Self::VmStackOverflow,
            -639 => Self::VmStackUnderflow,
            -640 => Self::VmInvalidOpCode,
            -641 => Self::VmInvalidExtraDataOnOpCode,
            -642 => Self::VmInvalidCommand,
            -643 => Self::VmFakeShortcutLogicalOperation,
            -644 => Self::VmDivideByZero,
            -645 => Self::VmFakeAbortScript,
            -646 => Self::VmIpOutOfCodeSegment,
            -647 => Self::VmCommandImplementerNotSet,
            -648 => Self::VmUnknownTypeOnRunTimeStack,
            -1 => Self::AlreadyPrinted,
            _ => return Err(UnknownCompilerErrorCode::new(code)),
        };
        Ok(value)
    }
}
