import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.util.StringTokenizer;

public final class Main {
    private Main() {}

    public static void main(String[] args) throws Exception {
        BufferedReader br = new BufferedReader(new InputStreamReader(System.in));
        String line = br.readLine();
        if (line == null) {
            return;
        }
        line = line.trim();
        switch (line) {
            case "simulate-timeout" -> System.out.println("[TIMEOUT]");
            case "crash" -> System.out.println("[EXCEPTION] Illegal Operation");
            default -> handleMath(line);
        }
    }

    private static void handleMath(String line) {
        StringTokenizer st = new StringTokenizer(line);
        if (st.countTokens() != 2) {
            System.out.println("[ERROR] invalid input");
            return;
        }
        int lhs = Integer.parseInt(st.nextToken());
        int rhs = Integer.parseInt(st.nextToken());
        try {
            // Trigger helper usage; failure falls into the catch below.
            MathHelper.safeDivide(lhs, rhs);
            System.out.println(lhs + rhs);
        } catch (ArithmeticException ex) {
            System.out.println("[ERROR] " + ex.getMessage());
        }
    }
}
