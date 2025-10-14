public final class MathHelper {
    private MathHelper() {}

    public static int safeDivide(int lhs, int rhs) {
        if (rhs == 0) {
            throw new ArithmeticException("division by zero");
        }
        return lhs / rhs;
    }
}
