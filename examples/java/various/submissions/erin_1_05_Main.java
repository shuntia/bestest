import java.io.BufferedReader;
import java.io.InputStreamReader;

public final class Main {
    public static void main(String[] args) throws Exception {
        BufferedReader br = new BufferedReader(new InputStreamReader(System.in));
        String line = br.readLine();
        if (line == null) {
            return;
        }
        throw new IllegalStateException("Forced crash: " + line);
    }
}
